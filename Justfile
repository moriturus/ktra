[private]
@default:
	echo Dockerless build system for Ktra
	echo
	just --list

# Builds Ktra.
build:
	cargo build
