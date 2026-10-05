# the store: where the owner finds apps. rift-store is in the workspace package base.nix installs,
# so this is its row in the applications menu and nothing else. the apps it installs go into the
# system installation, the drive's @flatpak volume, which welcome.nix already gives flathub as a
# remote and which flatpak's own polkit rule allows the owner to install into and take apps out
# without a password
{ pkgs, ... }:
{
  environment.systemPackages = [
    (pkgs.makeDesktopItem {
      name = "dev.rift.Store";
      desktopName = "Store";
      comment = "Apps from Flathub, with what each one asks for";
      exec = "rift-store";
      icon = "system-software-install-symbolic";
      categories = [ "System" ];
    })
  ];
}
