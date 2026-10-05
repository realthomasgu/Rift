# an appimage: one file a person downloads and runs. it is the escape hatch of the install model,
# not a source a store can list, so nothing here is a catalogue. what it takes is an ordinary
# linux root: the program inside an appimage asks for /lib64/ld-linux-x86-64.so.2 and finds its
# libraries under /usr/lib, and a nix system has neither, so without this nothing in one runs at
# all. appimage-run lays those folders out over the store, read only, and runs the file in them.
# the two binfmt registrations give the kernel the first bytes of a type 1 and of a type 2, so a
# file a person makes executable and runs runs through appimage-run by itself, which is what every
# page about appimages says to do. programs.appimage turns programs.fuse on with it: the fusermount
# wrappers were already here, flatpak brought them
{
  programs.appimage = {
    enable = true;
    binfmt = true;
  };
}
