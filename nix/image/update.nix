# what a version is published as. two ways in, one shape of result (ADR-0089).
#
# the whole files systemd-sysupdate installs a version from (ab-sysupdate.nix): the store and its
# verity partition cut out of the image, the uki, and SHA256SUMS over them. a partition's uuid is in
# its file name. sysupdate gives the partition it writes that uuid, and the uki finds the store by it.
#
# and beside them the index of the store and of the uki, the root hash of the verity tree, and the
# chunks the two are made of. an index is the list of chunks a file is made of, each named by its
# hash; the chunks are ordinary files named by that hash, in one directory that holds every version's,
# so a chunk two versions share is stored once. `rift update` fetches the index, seeds from the slot
# it is running, fetches only the chunks it is missing, writes the free slot itself and makes the
# verity tree there, which is why the tree is not published at all
{
  config,
  pkgs,
  # the chunk store of this version. off for a version that is only ever installed from the whole
  # files, which saves a runner 5 GiB of disk for nothing
  chunks ? true,
}:
let
  inherit (config.system.image) id version;
  inherit (config.system.build) intermediateImage uki;
  inherit (config.system.boot.loader) ukiFile;
  # the image before its esp is written. the partitions sysupdate reads are the same in both
  raw = "${intermediateImage}/${config.image.baseName}.raw";
in
pkgs.runCommand "${id}-update-${version}"
  {
    nativeBuildInputs = [
      pkgs.jq
      pkgs.zstd
    ]
    ++ pkgs.lib.optional chunks pkgs.desync;
    __structuredAttrs = true;
    # the uki names store paths, but these files are a system of their own and keep nothing alive
    unsafeDiscardReferences.out = true;
  }
  ''
    mkdir "$out"
    repart=${intermediateImage}/repart-output.json
    # what repart says about one partition of the image: its uuid, where it lies and how long it is
    about() {
      jq -r -e --arg type "$1" \
        '.[] | select(.type == $type) | "\(.uuid) \(.offset) \(.raw_size)"' "$repart" || {
        echo "the image has no $1 partition" >&2
        exit 1
      }
    }
    # one partition cut out of the image, as the file it is published as
    cut() {
      local offset=$1 size=$2 to=$3
      dd if=${raw} iflag=skip_bytes,count_bytes skip="$offset" count="$size" bs=4M status=none of="$to"
    }

    read -r verity_uuid verity_offset verity_size < <(about usr-x86-64-verity)
    read -r store_uuid store_offset store_size < <(about usr-x86-64)
    echo "store: partition $store_uuid, $store_size bytes at $store_offset"
    echo "verity: partition $verity_uuid, $verity_size bytes at $verity_offset"

    # the store and its tree as whole files, which is what systemd-sysupdate installs from. the
    # store is an erofs that zstd has already compressed, so this gains about a tenth
    cut "$store_offset" "$store_size" store.raw
    zstd -q -T"$NIX_BUILD_CORES" -12 store.raw -o "$out/${id}_${version}_$store_uuid.store.zst"
    cut "$verity_offset" "$verity_size" verity.raw
    zstd -q -T"$NIX_BUILD_CORES" -12 verity.raw -o "$out/${id}_${version}_$verity_uuid.verity.zst"
    rm verity.raw
    cp ${uki}/${ukiFile} "$out/${id}_${version}.efi"

    # the root hash of the pair. the store and its tree both carry it, so it is the one value they
    # agree on, and the uki names it on its command line. a drive that makes the tree itself proves
    # with it that the store image it assembled is the one the hash was signed over
    jq -r -e '
      map(select(.roothash != null) | .roothash) | unique
      | if length == 1 then .[0]
        else error("expected one root hash in the image, found \(length)") end
    ' "$repart" > "$out/${id}_${version}.store.roothash"
    echo "root hash: $(cat "$out/${id}_${version}.store.roothash")"

    ${
      if chunks then
        ''
          # the index of each file and the chunks it is made of, at desync's own sizes: 16 KiB least,
          # 64 KiB on average, 256 KiB most. two neighbouring store images share nine chunks in ten
          mkdir -p "$out/chunks"
          desync make -n "$NIX_BUILD_CORES" -m 16:64:256 -s "$out/chunks" \
            "$out/${id}_${version}_$store_uuid.store.caibx" store.raw
          desync make -n "$NIX_BUILD_CORES" -m 16:64:256 -s "$out/chunks" \
            "$out/${id}_${version}.efi.caibx" "$out/${id}_${version}.efi"
          echo "chunks: $(find "$out/chunks" -type f | wc -l)"
          du -sh "$out/chunks"
        ''
      else
        ''
          echo "chunks: none, this version is installed from its whole files"
        ''
    }
    rm store.raw

    # the indexes, the root hash and the whole files. the chunks need no hash of their own: the
    # index names every one of them by its hash
    cd "$out"
    sha256sum ${id}_${version}* > SHA256SUMS
    cat SHA256SUMS
  ''
