# an appimage of our own for the boot test, so the test needs nothing from the internet and the
# file it runs is the shape a real one is.
#
# an appimage is two things in one file: an elf program, the runtime, with the letters AI and the
# type in its header where the elf abi version and two of its padding bytes sit, and a squashfs
# image appended at the end of the section headers. appimage-run reads both out of the header the
# same way, with readelf, and unpacks the squashfs at that offset; the kernel's binfmt rule reads
# the first eleven bytes. the runtime here is a static program that says what the file is and
# nothing else: on rift the kernel hands the file to appimage-run and never runs it, and a build
# that bundled upstream's runtime would be testing upstream's runtime.
#
# what is inside is a real appimage's shape as well: an AppRun script at the top that runs the
# program under usr/bin, and that program is linked the way every app in an appimage is, asking
# for the loader at /lib64/ld-linux-x86-64.so.2 and for its libraries by name alone. nothing on
# the drive has those folders, so it runs in appimage-run's and nowhere else, which is the whole
# question this test answers
{ pkgs }:
let
  name = "rift-test.AppImage";
  said = "rift test appimage ran";
  # the program inside: it says its line and writes the same line into the file the test looks
  # for, since the file manager starts an app with its output going nowhere
  program = pkgs.writeText "rift-test-appimage.c" ''
    #include <stdio.h>
    #include <stdlib.h>
    #include <string.h>

    int main(void) {
      printf("${said}\n");
      const char *home = getenv("HOME");
      if (home != NULL && strlen(home) < 3000) {
        char path[4096];
        snprintf(path, sizeof path, "%s/appimage-ran.txt", home);
        FILE *file = fopen(path, "w");
        if (file != NULL) {
          fprintf(file, "${said}\n");
          fclose(file);
        }
      }
      return 0;
    }
  '';
  # the runtime: what a person sees if anything ever runs the file itself
  runtime = pkgs.writeText "rift-test-appimage-runtime.c" ''
    #include <stdio.h>

    int main(void) {
      fprintf(stderr, "This file is an AppImage. Run it with appimage-run.\n");
      return 1;
    }
  '';
  apprun = pkgs.writeText "AppRun" ''
    #!/bin/sh
    here=$(dirname "$(readlink -f "$0")")
    exec "$here/usr/bin/rift-test-appimage" "$@"
  '';
in
pkgs.runCommandCC "rift-test-appimage"
  {
    nativeBuildInputs = [
      pkgs.squashfsTools
      pkgs.patchelf
      pkgs.binutils
    ];
    passthru = { inherit name said; };
  }
  ''
    mkdir -p root/usr/bin

    # the program, linked the way an appimage's is: the loader by its fhs path, and no rpath, so
    # it finds libc under /usr/lib or nowhere
    $CC -O2 -o root/usr/bin/rift-test-appimage ${program}
    patchelf --set-interpreter /lib64/ld-linux-x86-64.so.2 --remove-rpath root/usr/bin/rift-test-appimage
    cp ${apprun} root/AppRun
    chmod +x root/AppRun
    mksquashfs root payload.squashfs -noappend -no-xattrs -all-root -quiet -no-progress

    # the runtime, with the appimage signature written over the abi version and two padding
    # bytes of its elf header: 41 49 is AI and 02 is a type 2 appimage
    $CC -static -O2 -o runtime ${runtime}
    chmod +w runtime
    printf '\x41\x49\x02' | dd of=runtime bs=1 seek=8 conv=notrunc status=none

    # the offset appimage-run unpacks at is the end of the section headers, which is the end of
    # the file for a program linked here. a build where it is not would write a file nothing can
    # unpack, so it stops here instead
    offset=$(LC_ALL=C readelf -h runtime |
      awk 'NR==13 { shoff = $5 } NR==18 { size = $5 } NR==19 { count = $5 } END { print shoff + size * count }')
    size=$(stat -c %s runtime)
    if [ "$offset" != "$size" ]; then
      echo "the runtime's section headers end at $offset, not at its end, $size" >&2
      exit 1
    fi

    mkdir -p $out
    cat runtime payload.squashfs > $out/${name}
    chmod +x $out/${name}
  ''
