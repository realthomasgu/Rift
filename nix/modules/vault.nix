# vault: timeline snapshots of home, rustic backups, drive cloning, and the owner's name and
# password. vault serve answers on the system bus as dev.rift.Vault and a timer takes a snapshot
# every hour. backups go to a folder on another disk that sudo vault target chooses. sudo rift clone
# writes a second drive onto a removable disk, as root in the terminal, not through the service
{
  config,
  lib,
  pkgs,
  self,
  ...
}:
let
  cfg = config.rift.vault;
  busName = "dev.rift.Vault";
  # anyone on the machine may list, take and restore. a restore runs as the account that asked,
  # and every take runs the retention rules. only root owns the name
  policy = pkgs.writeTextFile {
    name = "vault-dbus-policy";
    destination = "/share/dbus-1/system.d/${busName}.conf";
    text = ''
      <!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
       "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
      <busconfig>
        <policy user="root">
          <allow own="${busName}"/>
        </policy>
        <policy context="default">
          <allow send_destination="${busName}" send_interface="${busName}"/>
          <allow send_destination="${busName}" send_interface="org.freedesktop.DBus.Introspectable"/>
          <allow send_destination="${busName}" send_interface="org.freedesktop.DBus.Peer"/>
        </policy>
      </busconfig>
    '';
  };
  # the service and the timer use the same places and the same rules
  timeline = lib.concatStringsSep " " [
    "--subvolume /persist/@home"
    "--snapshots /persist/@snapshots/home"
    "--hourly ${toString cfg.timeline.hourly}"
    "--daily ${toString cfg.timeline.daily}"
    "--weekly ${toString cfg.timeline.weekly}"
  ];
  keepOption =
    default: what:
    lib.mkOption {
      type = lib.types.ints.unsigned;
      inherit default;
      description = "How many of the latest ${what} keep their first snapshot.";
    };
in
{
  options.rift.vault = {
    enable = lib.mkEnableOption "Vault, snapshots and backups";
    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.workspace;
      description = "The build that provides the vault binary.";
    };
    timeline = {
      hourly = keepOption 24 "hours";
      daily = keepOption 7 "days";
      weekly = keepOption 8 "weeks";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = with pkgs; [
      rustic
      btrfs-progs
      # a clone makes the new drive's esp, exchange and persist
      cryptsetup
      dosfstools
      exfatprogs
    ];
    # usb flash lies about its health, scrub monthly
    services.btrfs.autoScrub = {
      enable = true;
      fileSystems = [ "/persist" ];
      interval = "monthly";
    };
    services.dbus.packages = [ policy ];
    # cryptsetup's luks2 lock directory, which vault writes the header of persist under
    systemd.tmpfiles.rules = [ "d /run/cryptsetup 0700 root root -" ];

    systemd.services.vault = {
      description = "Vault";
      wantedBy = [ "multi-user.target" ];
      requires = [ "dbus.service" ];
      after = [
        "local-fs.target"
        "dbus.service"
      ];
      unitConfig.RequiresMountsFor = [
        "/persist"
        "/home"
      ];
      path = [
        pkgs.btrfs-progs
        pkgs.rustic
        pkgs.util-linux
        # reading the header of persist, to see whether a key is sealed to this machine's tpm.
        # systemd-cryptenroll, which seals and wipes it, comes with systemd on the default path
        pkgs.cryptsetup
      ];
      serviceConfig = {
        Type = "dbus";
        BusName = busName;
        ExecStart = "${cfg.package}/bin/vault serve --home /home ${timeline}";
        Restart = "on-failure";
        # root, because taking and deleting a snapshot needs it, and so do mounting the backup disk
        # and reading all of home for a backup. a restore reads and writes home in a child that
        # runs as the account that asked
        ProtectSystem = "strict";
        ReadWritePaths = [
          "/persist"
          "/home"
          # cryptsetup takes its luks2 lock here, and strict would otherwise hand this service a
          # read-only /run. without it nothing can write the header of persist. the rule below
          # makes the directory, since cryptsetup's own tmpfiles rule is not ours to rely on
          "-/run/cryptsetup"
        ];
        # the backup target and its password, and the owner's own name and the hash of their
        # password. only root reads them
        StateDirectory = [
          "rift/vault"
          "rift/owner"
        ];
        StateDirectoryMode = "0700";
        # a restore from a backup lands here before the copy into home
        CacheDirectory = "vault";
        # the backup disk is mounted under here, in this service's own mount namespace, and goes
        # away with it
        RuntimeDirectory = "vault";
        PrivateTmp = true;
        # the bus is a unix socket, so it is still there without a network
        PrivateNetwork = true;
        NoNewPrivileges = true;
      };
    };

    # the owner's name and password, which the Owner page keeps on persist through vault. root is
    # a tmpfs, so nixos makes the account again at every boot with the image's name; this puts the
    # owner's own name and password into the password files before anyone logs in, and vault starts
    # it again after a change, so the change is there at once. it is the one part of vault that
    # writes to /etc
    systemd.services.vault-owner = {
      description = "Vault, the owner's name and password";
      wantedBy = [ "multi-user.target" ];
      before = [ "systemd-user-sessions.service" ];
      unitConfig.RequiresMountsFor = [ "/var/lib/rift" ];
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "${cfg.package}/bin/vault owner";
        ProtectSystem = "strict";
        ReadWritePaths = [ "/etc" ];
        ProtectHome = true;
        PrivateTmp = true;
        PrivateNetwork = true;
        NoNewPrivileges = true;
      };
    };

    # the drive's own exchange partition, the plain one another computer can read. udisks does
    # not mount it: on a real stick it would let the owner mount every partition of the drive,
    # and in a virtual machine, where the drive is an internal disk, it refuses the mount
    # outright. it is the drive's own, so the system mounts it, before anyone logs in. nothing
    # here is sandboxed: a mount made inside a mount namespace of its own would be invisible to
    # the rest of the system, which is the whole point of making it
    systemd.services.vault-exchange = {
      description = "Vault, the drive's exchange partition";
      wantedBy = [ "multi-user.target" ];
      after = [ "local-fs.target" ];
      before = [ "systemd-user-sessions.service" ];
      path = [ pkgs.util-linux ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = "${cfg.package}/bin/vault exchange";
        ExecStop = "-${pkgs.util-linux}/bin/umount /exchange";
      };
    };

    # the hourly snapshot. systemd catches up once at boot when the drive was off at the hour
    systemd.services.vault-timeline = {
      description = "Vault, the hourly snapshot of home";
      unitConfig.RequiresMountsFor = [ "/persist" ];
      path = [ pkgs.btrfs-progs ];
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "${cfg.package}/bin/vault take ${timeline}";
        ProtectSystem = "strict";
        ReadWritePaths = [ "/persist" ];
        ProtectHome = true;
        PrivateTmp = true;
        PrivateNetwork = true;
        NoNewPrivileges = true;
      };
    };
    systemd.timers.vault-timeline = {
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnCalendar = "hourly";
        Persistent = true;
      };
    };
  };
}
