# ghost mode: the drive's second boot entry. persist stays locked, /home is a tmpfs, and nothing
# the session does is on the drive when it stops. ADR-0082.
#
# the entry is a second profile inside the one uki, not a second uki and not a loader entry. a
# profile is a set of pe sections that take the place of the base's; systemd-boot draws one menu
# line per profile and hands the stub the profile number as the first word of the command line, so
# the two entries share a kernel, an initrd and a store, and differ by one word. the ghost profile
# takes over .cmdline alone, with rift.ghost and rd.luks=0 on the end of it.
#
# rift.ghost is the word everything of Rift's reads. A condition cannot do the job on its own:
# systemd reads a unit's conditions when it starts it, which is after its dependencies are
# satisfied, so a mount that is going to be skipped has already had a job enqueued for the device it
# names. Persist is never opened in a ghost boot, so that device never appears and the boot would
# end in emergency mode ninety seconds later. The mounts that come off persist are noauto instead
# (nix/image/persist.nix), so nothing pulls them in at all, and rift-persist.service below starts
# them on every boot that is not a ghost one.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  inherit (config.system.boot.loader) ukiFile;

  # the command line the base profile gets, the way nix/image's verity store builds it. the usrhash
  # is only known once the store's hash tree is built, so both profiles are written in one go
  cmdline = "init=${config.system.build.toplevel}/init ${toString config.boot.kernelParams}";
  # and the words that make a boot a ghost one.
  #
  # rd.luks=0 turns off the generator that would make the unit that opens persist, so nothing asks
  # for the passphrase and nothing touches the header. The masks are for the other side of the
  # switch: the mounts are noauto there too, so nothing requires them, but Vault, Orbit and Quasar
  # each name a path on persist in RequiresMountsFor=, and a masked unit makes them fail at once
  # instead of waiting ninety seconds for a device that is not coming
  words = lib.concatStringsSep " " (
    [
      "rift.ghost"
      "rd.luks=0"
    ]
    ++ map (name: "systemd.mask=${name}.mount") persistMounts
  );

  # the base profile names itself and nothing else: with no TITLE the menu line stays the drive's
  # own name, "Rift <version>", which is what an ordinary boot should look like
  baseProfile = pkgs.writeText "rift-profile" "ID=main\n";
  # and the ghost one gives systemd-boot the words for its line, "Rift <version> (Ghost mode)"
  ghostProfile = pkgs.writeText "rift-ghost-profile" ''
    ID=ghost
    TITLE=Ghost mode
  '';

  # the mounts that come off persist, under /sysroot in the initrd and at their own names after the
  # switch. a ghost boot has none of them: each one is a directory on the root tmpfs instead
  persistMounts = [
    "persist"
    "home"
    "var"
    "var-lib-flatpak"
    "var-lib-rift-models"
    "var-lib-rift-hosts"
  ];

  # the drop-in that keeps a unit out of a ghost boot. it works for these three because none of them
  # is required by anything: a condition is read late, so it can skip a unit but never stop one
  # being pulled in
  skipped = ''
    [Unit]
    ConditionKernelCommandLine=!rift.ghost
  '';
  # for the units nothing else of Rift's defines: /boot, which a generator makes, and systemd's own
  skip =
    names:
    lib.listToAttrs (
      map (
        name:
        lib.nameValuePair name {
          overrideStrategy = "asDropin";
          text = skipped;
        }
      ) names
    );
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
        # the store and its hash tree are a verity pair, and systemd-repart writes the same root
        # hash on both halves, so the hash is the one value they agree on. matching on the type
        # would not do: nix/image/default.nix configures it as usr-verity and repart writes the
        # architecture's own name, usr-x86-64-verity
        usrhash=$(jq -r -e '
          map(select(.roothash != null) | .roothash) | unique
          | if length == 1 then .[0]
            else error("expected one root hash in the image, found \(length)") end
        ' ${config.system.build.intermediateImage}/repart-output.json)

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
    # the places the subvolumes of persist would have been. /home is one of them: the root is a
    # tmpfs, so a directory in it is a tmpfs, which is what ADR-0013 asks for and needs no file
    # system of its own. they are made before the switch because /var/lib/rift holds the machine
    # id, which systemd reads through /etc/machine-id before any unit runs, and because a service
    # with ReadWritePaths= for a directory that is not there fails to start. each one is empty,
    # which is what a ghost boot has in place of what the drive keeps
    # it hangs off initrd.target and not initrd-fs.target, which a ghost boot masks
    services.ghost-mode = {
      description = "Ghost mode: the empty places of a locked persist";
      wantedBy = [ "initrd.target" ];
      before = [ "initrd-cleanup.service" ];
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
          "/bin/mkdir -p /sysroot/home /sysroot/persist /sysroot/var/lib/rift/hosts /sysroot/var/lib/rift/models /sysroot/var/lib/flatpak"
        ];
      };
    };

    # what mounts persist's subvolumes on every boot that is not a ghost one. They are noauto, so
    # nothing else pulls them in, and this is the one place a condition does the job: an ExecStart
    # only runs when the condition passes, where a dependency is enqueued whether it passes or not.
    # initrd-fs.target requires this service, so a mount that fails still stops the boot
    services.rift-persist = {
      description = "Mount what persist keeps";
      requiredBy = [ "initrd-fs.target" ];
      before = [ "initrd-fs.target" ];
      after = [ "cryptsetup.target" ];
      unitConfig = {
        ConditionKernelCommandLine = "!rift.ghost";
        DefaultDependencies = false;
      };
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = "/bin/systemctl start ${
          lib.concatMapStringsSep " " (name: "sysroot-${name}.mount") persistMounts
        }";
      };
    };

    # and nothing makes a persist either: vault-first-boot would make one on a drive without it.
    # Nothing requires this service, so a condition is enough to keep it out
    services.vault-first-boot.unitConfig.ConditionKernelCommandLine = "!rift.ghost";
  };

  systemd.units = skip [
    # blessing a version is a statement about the drive, and a ghost boot makes none. systemd-boot
    # still takes a try off the counter before the kernel starts, which is the firmware's own write
    # and the one thing a ghost boot cannot stop
    "systemd-bless-boot.service"
    # the esp is not mounted at all: it is the one part of the drive that is not encrypted, and a
    # vfat mounted for writing is written whether anything writes to it or not. /boot is nofail and
    # an automount, so nothing requires either of these and the condition does keep them out
    "boot.mount"
    "boot.automount"
  ];

  # it writes a fresh seed onto the esp and a token into the machine's own variables
  systemd.services.systemd-boot-random-seed.unitConfig.ConditionKernelCommandLine = "!rift.ghost";
  # the drive's exchange partition is not mounted either. it holds nothing personal, but mounting
  # it writes to it, and the one mode whose promise is that it writes nothing has to keep that
  systemd.services.vault-exchange.unitConfig.ConditionKernelCommandLine = "!rift.ghost";
  # and there is nothing to snapshot: home is a tmpfs and persist is locked
  systemd.services.vault-timeline.unitConfig.ConditionKernelCommandLine = "!rift.ghost";
  systemd.timers.vault-timeline.unitConfig.ConditionKernelCommandLine = "!rift.ghost";
}
