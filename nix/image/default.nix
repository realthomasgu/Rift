# liftoff: the read-only, verity-checked system image
#
# the image is slot a of the a/b layout: the esp with systemd-boot and a uki that counts its boots,
# the store's verity partition and the store. writing it to a drive adds slot b and persist behind it
# (rift-flash). ab-sysupdate.nix is the running side
{
  config,
  lib,
  pkgs,
  modulesPath,
  ...
}:
let
  inherit (config.system.image) id version;
  systemdBoot = "${config.systemd.package}/lib/systemd/boot/efi/systemd-bootx64.efi";
in
{
  imports = [
    "${modulesPath}/image/repart.nix"
    "${modulesPath}/image/repart-verity-store.nix"
    ./persist.nix
    ./ab-sysupdate.nix
    ./ghost.nix
  ];

  boot.loader.grub.enable = false;
  boot.initrd.systemd.enable = true;

  # names the uki, the image file and the slot labels: rift_<version>, store_<version>
  system.image.id = "rift";
  system.image.version = "0.1.0";

  # root is tmpfs. the store is the verity partition, everything personal is on persist, and in
  # ghost mode nothing is on persist, so home and var are in here too. half the memory is a
  # ceiling and not a reservation: a tmpfs holds only what is written to it, and zram swap takes
  # what it holds under pressure
  fileSystems."/" = {
    fsType = "tmpfs";
    options = [
      "mode=0755"
      "size=50%"
    ];
  };

  image.repart = {
    name = "rift";
    verityStore = {
      enable = true;
      # +3 is the boot counter: three tries before systemd-boot falls back to the other slot
      ukiPath = "/EFI/Linux/${id}_${version}+3.efi";
    };
    partitions = {
      "00-esp" = {
        # the firmware starts systemd-boot from the removable media path, nothing is written to its
        # variables, and the command line cannot be edited. the menu is drawn for three seconds at
        # every boot, because the drive has two entries since ADR-0082 and a key held down while the
        # firmware starts does not reach systemd-boot: the firmware's own boot stage reads the
        # keyboard until it hands the loader the machine. any key stops the countdown
        contents = {
          "/EFI/BOOT/BOOTX64.EFI".source = systemdBoot;
          "/EFI/systemd/systemd-bootx64.efi".source = systemdBoot;
          "/loader/loader.conf".source = pkgs.writeText "loader.conf" ''
            timeout 3
            editor no
          '';
        };
        repartConfig = {
          Type = "esp";
          Format = "vfat";
          Label = "esp";
          SizeMinBytes = "1G";
          SizeMaxBytes = "1G";
        };
      };
      # a fixed 1G, enough for the hash tree of a full 8G store, so a later version fits here too.
      # the tree is hashed in 4 KiB blocks, which is what veritysetup itself defaults to. repart
      # would take the block size from the image's sector size, 512, and a tree over 512 byte blocks
      # is eight times as big: 374 MiB of hashes to send with every version instead of 45 MiB
      "10-store-verity" = {
        repartConfig = {
          Type = "usr-verity";
          Label = "store-verity_${version}";
          Minimize = "off";
          SizeMinBytes = "1G";
          SizeMaxBytes = "1G";
          VerityDataBlockSizeBytes = "4096";
          VerityHashBlockSizeBytes = "4096";
        };
      };
      # as small as what is in it, and the last partition, so the flash step can grow it to its 8G slot.
      # erofs compresses it with zstd in clusters of up to 64 KiB, so a read decompresses little more
      # than it asked for. higher levels save a few percent and take many times as long to build
      "20-store" = {
        repartConfig = {
          Type = "usr";
          Label = "store_${version}";
          Minimize = "best";
          SizeMaxBytes = "8G";
          Compression = "zstd";
          CompressionLevel = "3";
        };
      };
    };
    mkfsOptions.erofs = [ "-C65536" ];
  };

  # no nix-daemon on the device yet: the store is read-only and root is tmpfs, the db wouldn't survive a boot.
  # a writable overlay store on persist comes later
  nix.enable = false;
}
