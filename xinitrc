DISPLAY=:1

xrandr --output HDMI-0 --mode 1920x1080 --left-of DP-0 --output DP-0 --primary -r 144 --mode 1920x1080 --pos 1920x0 --output DP-5 --mode 1920x1080 --right-of DP-0

xeyes &
xeyes &
sleep 2 && xterm &
# xeyes &
# xeyes &
# xeyes &
# xeyes &
# xeyes &

exec ./target/release/wwm
