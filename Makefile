# MagiDraw Windows build. Run `make windows` on Linux with the Rust Windows GNU
# target and MinGW installed. Local balance, adaptive detail, and line-art tracing
# are built in.
# Versioned executables and checksums go in dist/.

NAME           := magidraw
VERSION        := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
DIST           := dist
WINDOWS_TARGET := x86_64-pc-windows-gnu
WINDOWS_BIN    := $(DIST)/$(NAME)-$(VERSION)-windows-x86_64.exe
DEBUG_BIN      := $(DIST)/$(NAME)-debug-windows-x86_64.exe
CHECKSUMS      := $(DIST)/$(NAME)-$(VERSION)-checksums.txt
SOURCES        := Cargo.toml Cargo.lock $(wildcard src/*.rs)

.PHONY: all windows windows-debug checksums check clean

all: windows checksums

windows: $(WINDOWS_BIN)

$(WINDOWS_BIN): $(SOURCES)
	cargo build --locked --release --target $(WINDOWS_TARGET)
	@mkdir -p $(DIST)
	cp target/$(WINDOWS_TARGET)/release/$(NAME).exe $@
	@echo "[+] $@"

windows-debug:
	cargo build --locked --target $(WINDOWS_TARGET)
	@mkdir -p $(DIST)
	cp target/$(WINDOWS_TARGET)/debug/$(NAME).exe $(DEBUG_BIN)
	@echo "[+] $(DEBUG_BIN)"

checksums: $(WINDOWS_BIN)
	@cd $(DIST) && sha256sum $(notdir $(WINDOWS_BIN)) > $(notdir $(CHECKSUMS))
	@echo "[+] $(CHECKSUMS)"

check:
	cargo fmt -- --check
	cargo clippy --locked --release --target $(WINDOWS_TARGET) -- -D warnings

clean:
	cargo clean
	rm -rf $(DIST)
