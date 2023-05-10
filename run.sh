#! /usr/bin/env bash
DISPLAY=":1"

cargo build --release

xinit ./xinitrc -- /sbin/Xorg :1

