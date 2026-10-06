# hw/

One file per machine Rift has booted on, named vendor-model.md, in the shape of TEMPLATE.md.

`rift doctor --report` on the machine writes one. It fills in every fact, the verdict, the checks,
and the PCI and USB devices, and says on stderr which file name it wants:

    rift doctor --report > hw/<vendor>-<model>.md

What it cannot know goes in by hand: what worked, what didn't, and the quirks under Notes for
Orbit. A machine that does not boot gets a file written by hand on another one.

The hardware page of the website is built from these files with `just hw-site <page>`.
Anything under private/ stays out of git.
