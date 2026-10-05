# a flatpak runtime and two apps of our own for the boot test, in a repository the test serves and
# adds as a remote, so welcome and the store install them the way they install one from flathub and
# the test needs nothing from flathub. the runtime is a static busybox and a static gdbus, each app
# one shell script. the second app carries appstream data of its own, which is what a store searches
# and reads a name and a line about an app out of, and asks for a few things, which is what its page
# shows before it is installed
{ pkgs }:
let
  inherit (pkgs) lib;
  runtime = "dev.rift.TestPlatform";
  app = "dev.rift.TestApp";
  editor = "dev.rift.TestEditor";
  # the names the applications menu lists the installed apps under
  appName = "Rift test app";
  editorName = "Rift test editor";
  editorAbout = "A plain text editor for the boot test";
  branch = "test";
  # what the app does: reads the file the document portal gave it, then the same file where it is in
  # home, asks the desktop portal whether there is a network, and asks the user manager for
  # something, which the bus proxy flatpak puts in front of the session bus does not pass on
  probe = pkgs.writeText "probe" ''
    #!/bin/sh
    document=$1
    direct=$2
    echo "cgroup: $(cat /proc/self/cgroup)"
    cat "$document" && echo document-read
    cat "$direct" && echo direct-read
    gdbus call --session --timeout 10 --dest org.freedesktop.portal.Desktop \
      --object-path /org/freedesktop/portal/desktop \
      --method org.freedesktop.portal.NetworkMonitor.GetAvailable && echo portal-answered
    gdbus call --session --timeout 10 --dest org.freedesktop.systemd1 \
      --object-path /org/freedesktop/systemd1 \
      --method org.freedesktop.DBus.Peer.Ping && echo manager-answered
    echo finished
  '';
in
pkgs.runCommand "rift-test-flatpak"
  {
    nativeBuildInputs = [
      pkgs.flatpak
      pkgs.gnupg
    ];
  }
  ''
    export HOME=$TMPDIR

    # the system installation takes nothing over http from a remote that is not signed, so the
    # repository is, with a key made here and thrown away with the build. the test imports the public
    # half when it adds the remote
    export GNUPGHOME=$TMPDIR/gnupg
    mkdir -m 700 $GNUPGHOME
    gpg --batch --pinentry-mode loopback --passphrase "" \
      --quick-generate-key "Rift test <test@rift.invalid>" rsa2048 sign never
    key=$(gpg --list-keys --with-colons | awk -F: '/^fpr/ { print $10; exit }')
    sign="--gpg-sign=$key --gpg-homedir=$GNUPGHOME"

    # a runtime's files are its usr, and build-export wants the files folder a build-init makes as well
    mkdir -p platform/usr/bin platform/files
    cp ${pkgs.pkgsStatic.busybox}/bin/busybox platform/usr/bin/busybox
    for tool in $(platform/usr/bin/busybox --list); do
      [ -e platform/usr/bin/$tool ] || ln -s busybox platform/usr/bin/$tool
    done
    cp ${lib.getBin pkgs.pkgsStatic.glib}/bin/gdbus platform/usr/bin/gdbus
    cat > platform/metadata <<EOF
    [Runtime]
    name=${runtime}
    runtime=${runtime}/x86_64/${branch}
    sdk=${runtime}/x86_64/${branch}
    EOF
    flatpak build-export $sign --runtime --disable-fsync repo platform ${branch}

    mkdir -p testapp/files/bin testapp/export/share/applications
    install -m 755 ${probe} testapp/files/bin/probe
    # an exported desktop entry, so the test can see an installed flatpak in the applications menu
    cat > testapp/export/share/applications/${app}.desktop <<EOF
    [Desktop Entry]
    Type=Application
    Name=${appName}
    Exec=probe
    Icon=${app}
    Categories=Utility;
    EOF
    cat > testapp/metadata <<EOF
    [Application]
    name=${app}
    runtime=${runtime}/x86_64/${branch}
    sdk=${runtime}/x86_64/${branch}
    command=probe

    [Context]
    shared=network;
    EOF
    flatpak build-export $sign --disable-fsync repo testapp ${branch}

    # the second app. build-export reads appstream out of files/share/app-info and nowhere else, so
    # the components file is written here the way appstream-compose would write it; without it a
    # client knows the app by the last part of its id and nothing more
    mkdir -p editor/files/bin editor/files/share/app-info/xmls editor/export/share/applications
    printf '#!/bin/sh\necho ${editor}\n' > editor/files/bin/edit
    chmod 755 editor/files/bin/edit
    cat > components.xml <<EOF
    <?xml version="1.0" encoding="UTF-8"?>
    <components version="0.8">
      <component type="desktop-application">
        <id>${editor}.desktop</id>
        <name>${editorName}</name>
        <summary>${editorAbout}</summary>
        <description><p>The app the boot test installs from the Store. It has a name and a line about it of its own, so a search has something to find.</p></description>
        <project_license>GPL-3.0-or-later</project_license>
        <metadata_license>CC0-1.0</metadata_license>
        <categories><category>Utility</category><category>TextEditor</category></categories>
      </component>
    </components>
    EOF
    gzip -n -c components.xml > editor/files/share/app-info/xmls/${editor}.xml.gz
    cat > editor/export/share/applications/${editor}.desktop <<EOF
    [Desktop Entry]
    Type=Application
    Name=${editorName}
    Exec=edit
    Icon=${editor}
    Categories=Utility;TextEditor;
    EOF
    # a few things to ask for, one of them wide: the store's page says each of them in its own
    # sentence, the wide one first
    cat > editor/metadata <<EOF
    [Application]
    name=${editor}
    runtime=${runtime}/x86_64/${branch}
    sdk=${runtime}/x86_64/${branch}
    command=edit

    [Context]
    shared=network;ipc;
    sockets=wayland;pulseaudio;
    devices=dri;
    filesystems=home;xdg-download:ro;

    [Session Bus Policy]
    org.freedesktop.Notifications=talk
    EOF
    flatpak build-export $sign --disable-fsync repo editor ${branch}

    # the summary a client reads first, and the appstream branch a store searches, both signed
    flatpak build-update-repo $sign repo

    mkdir -p $out
    cp -r repo $out/repo
    gpg --export $key > $out/key.gpg
    gpgconf --kill gpg-agent
  ''
