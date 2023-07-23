# WWM: Wally's Window Manager
A simple non-reparenting dynamic window manager for X.

## Screenshots
__Tiling layout__:
![tiling layout](./screenshots/screenshot-1.png)

__Column layout__:
![column layout](./screenshots/screenshot-2.png)

## Requirements
- [fontconfig](https://www.freedesktop.org/wiki/Software/fontconfig/) to be able to discover installed fonts
- [xrandr](https://wiki.archlinux.org/title/Xrandr) to configure your monitor setup
- [feh](https://github.com/derf/feh) for setting wallpapers

## Installation
1. Clone repo `git clone https://github.com/mWalrus/wwm`
2. `cd wwm`
3. `sudo make install`
4. exit your current session and select `wwm` from your display manager

## Features
- [x] Multi monitor support using RandR
- [x] Workspaces/virtual desktops
- [x] Layouts
  - [x] Main-stack
  - [x] Column
  - [x] Floating (dialog windows, etc.)
- [ ] Bar
  - [x] Workspace tags
    - [x] Focus indication
    - [x] "Contains-clients" indication
    - [x] Click to change focus
  - [x] Current layout indicator
  - [x] Current focused window title
  - [ ] Modular status indicators (such as time, date, ram, cpu, etc.)
- [x] Cursor warping on client focus change
- [x] Customizability (configure in code)
  - [x] Theming
  - [x] Custom keybinds
  - [x] Auto start commands
  - [x] Program spawning
- [x] Move clients between monitors and/or workspaces
- [x] Respects floating clients such as dialog windows
- [x] Move clients with mouse
- [x] Resize clients with mouse
- [x] Unfloat floating clients
- [x] Fullscreening


## Configuration
All configuration is done in code in the [src/config.rs](./src/config.rs) file and
most of it should be pretty self explanatory in there.

## Development
1. On Linux you can enter another session using `Ctrl+Alt+F{3,4,5,...}`.
2. Once in a new session, log into it and go to the project root.
3. Run `RUST_BACKTRACE=full ./run.sh 2&>run.log`
    - This runs `xorg` through `xinit` followed by the window manager
    - It also logs all the errors and the backtrace to a new file `run.log` which is placed in
      the project root
