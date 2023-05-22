clean:
	cargo clean

install:
	cargo build --release
	mkdir -p /usr/share/xsessions
	cp -f wwm.desktop /usr/share/xsessions/
	chmod 644 /usr/share/xsessions/wwm.desktop
	mkdir -p /usr/local/bin
	cp -f ./target/release/wwm /usr/local/bin/
	chmod 755 /usr/local/bin/wwm
	mkdir -p /usr/share/wwm
	cp -f wallpaper.png /usr/share/wwm/

uninstall:
	rm -f /usr/local/bin/wwm
	rm -f /usr/share/xsessions/wwm.desktop
	rm -rf /usr/share/wwm

