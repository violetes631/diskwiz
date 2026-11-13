# Plain `make install` puts everything under /usr/local; use PREFIX=$HOME/.local for a user install.
PREFIX  ?= /usr/local
DESTDIR ?=
ID       = io.github.violetes631.Diskwiz

all:
	cargo build --release --locked

test:
	cargo test --locked

install: all
	install -Dm755 target/release/diskwiz         $(DESTDIR)$(PREFIX)/bin/diskwiz
	install -Dm644 data/$(ID).desktop             $(DESTDIR)$(PREFIX)/share/applications/$(ID).desktop
	install -Dm644 data/$(ID).svg                 $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/$(ID).svg
	-gtk-update-icon-cache -q -t $(DESTDIR)$(PREFIX)/share/icons/hicolor 2>/dev/null
	-update-desktop-database -q $(DESTDIR)$(PREFIX)/share/applications 2>/dev/null

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/diskwiz \
	      $(DESTDIR)$(PREFIX)/share/applications/$(ID).desktop \
	      $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/$(ID).svg

.PHONY: all test install uninstall
