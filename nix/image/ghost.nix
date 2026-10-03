# ghost mode: the drive's second boot entry. persist stays locked, /home is a tmpfs, and nothing
# the session does is on the drive when it stops. ADR-0082.
#
# the entry is a second profile inside the one uki, not a second uki and not a loader entry. a
# profile is a set of pe sections that take the place of the base's; systemd-boot draws one menu
# line per profile and hands the stub the profile number as the first word of the command line, so
# the two entries share a kernel, an initrd and a store, and differ by one word. the ghost profile
# takes over .cmdline alone, with rift.ghost and rd.luks=0 on the end of it.
#
# rift.ghost is the word everything of Rift's reads. rd.luks=0 is systemd's own switch for the
# generator that would otherwise make the unit that opens persist: with it there is no such unit,
# so nothing asks for the passphrase and nothing touches the header.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  inherit (config.system.boot.loader) ukiFile;
  inherit (config.image.repart) verityStore;
  verityType = config.image.repart.partitions.${verityStore.partitionIds.store-verity}.repartConfig.Type;

  # the command line the base profile gets, the way nix/image's verity store builds it. the usrhash
  # is only known once the store's hash tree is built, so both profiles are written in one go
  cmdline = "init=${config.system.build.toplevel}/init ${toString config.boot.kernelParams}";
  # and the words that make a boot a ghost one
  words = "rift.ghost rd.luks=0";

  # the base profile names itself and nothing else: with no TITLE the menu line stays the drive's
  # own name, "Rift <version>", which is what an ordinary boot should look like
  baseProfile = pkgs.writeText "rift-profile" "ID=main\n";
  # and the ghost one gives systemd-boot the words for its line, "Rift <version> (Ghost mode)"
  ghostProfile = pkgs.writeText "rift-ghost-profile" ''
    ID=ghost
    TITLE=Ghost mode
  '';

  # the mounts that come off persist, under /sysroot in the initrd and at their own names after the
  # switch. a ghost boot has none of them: /home is a tmpfs of its own and the rest are directories
  # on the root tmpfs
  persistMounts = [
    "persist"
    "home"
    "var"
    "var-lib-flatpak"
    "var-lib-rift-models"
    "var-lib-rift-hosts"
  ];

  # the drop-in that keeps a unit out of a ghost boot. one condition is the whole of what a unit
  # has to know about the mode
  skipped = ''
    [Unit]
    ConditionKernelCommandLine=!rift.ghost
  '';
  # for the units nothing else of Rift's defines: the mounts a generator makes, and systemd's own
  skip = names: lib.listToAttrs (map (name: lib.nameValuePair name { overrideStrategy = "asDropin"; text = skipped; }) names);
in
{
  # one uki, two profiles. this replaces the single profile one the verity store module builds,
  # which is why the usrhash is read again here: the base profile's command line has to carry it
  # for the image build's own check, and the ghost profile's has to carry the same one
  system.build.uki = lib.mkForce (
    pkgs.runCommand ukiFile
      {
        nativeBuildInputs = [
          pkgs.buildPackages.jq
          pkgs.buildPackages.systemdUkify
        ];
      }
      ''
        mkdir -p $out
        usrhash=$(jq -r \
          '.[] | select(.type=="${verityType}") | .roothash' \
          ${config.system.build.intermediateImage}/repart-output.json
        )

        # a profile binary holds no kernel: only the sections that differ from the base, with
        # .profile first in it
        ukify build \
          --profile=@${ghostProfile} \
          --cmdline="${cmdline} usrhash=$usrhash ${words}" \
          --output=ghost-profile.efi

        # the base's own .profile goes last of the base's sections, so everything before it is
        # shared, and the joined one becomes profile 1
        ukify build \
          --config=${config.boot.uki.configFile} \
          --cmdline="${cmdline} usrhash=$usrhash" \
          --profile=@${baseProfile} \
          --join-profile=ghost-profile.efi \
          --output="$out/${ukiFile}"
      ''
  );

  boot.initrd.systemd = {
    # /home is a tmpfs, mounted before the switch so nothing ever sees the empty directory under it.
    # half the memory is a ceiling and not a reservation: a tmpfs holds only what is written to it
    mounts = [
      {
        where = "/sysroot/home";
        what = "tmpfs";
        type = "tmpfs";
        options = "mode=0755,nosuid,nodev,size=50%";
        unitConfig.ConditionKernelCommandLine = "rift.ghost";
        wantedBy = [ "initrd-fs.target" ];
        before = [ "initrd-fs.target" ];
      }
    ];

    # the directories the subvolumes of persist would have been. they are made before the switch
    # because /var/lib/rift holds the machine id, which systemd reads through /etc/machine-id before
    # any unit runs, and because a service with ReadWritePaths= for a directory that is not there
    # fails to start. each one is empty, which is what a ghost boot has instead of what is kept
    services.ghost-mode = {
      description = "Ghost mode: the empty places of a locked persist";
      wantedBy = [ "initrd-fs.target" ];
      before = [ "initrd-fs.target" ];
      after = [ "sysroot.mount" ];
      unitConfig = {
        ConditionKernelCommandLine = "rift.ghost";
        DefaultDependencies = false;
        RequiresMountsFor = "/sysroot";
      };
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = [
          "/bin/mkdir -p /sysroot/persist /sysroot/var/lib/rift/hosts /sysroot/var/lib/rift/models /sysroot/var/lib/flatpak"
        ];
      };
    };

    # and nothing that reads or writes persist happens: vault-first-boot would make one on a drive
    # without it, and the mounts would wait for a mapper device that is never opened
    services.vault-first-boot.unitConfig.ConditionKernelCommandLine = "!rift.ghost";
    units = skip (map (name: "sysroot-${name}.mount") persistMounts);
  };

  # the same mounts on the other side of the switch: nixos puts every file system in /etc/fstab, so
  # the ones that were skipped in the initrd would be tried again here
  systemd.units = skip (map (name: "${name}.mount") persistMounts ++ [
    # blessing a version is a statement about the drive, and a ghost boot makes none. systemd-boot
    # still takes a try off the counter before the kernel starts, which is the firmware's own write
    # and the one thing a ghost boot cannot stop
    "systemd-bless-boot.service"
    # it writes a fresh seed onto the esp and a token into the machine's own variables
    "systemd-boot-random-seed.service"
    # the esp is not mounted at all: it is the one part of the drive that is not encrypted, and a
    # vfat mounted for writing is written whether anything writes to it or not
    "boot.mount"
    "boot.automount"
  ]);

  # the drive's exchange partition is not mounted either. it holds nothing personal, but mounting
  # it writes to it, and the one mode whose promise is that it writes nothing has to keep that
  systemd.services.vault-exchange.unitConfig.ConditionKernelCommandLine = "!rift.ghost";
  # and there is nothing to snapshot: home is a tmpfs and persist is locked
  systemd.services.vault-timeline.unitConfig.ConditionKernelCommandLine = "!rift.ghost";
  systemd.timers.vault-timeline.unitConfig.ConditionKernelCommandLine = "!rift.ghost";
}
