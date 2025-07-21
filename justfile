#!/usr/bin/env -S just --justfile

set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]
set shell := ["bash", "-cu"]

_default:
  @just --list -u

alias r := ready

ready:
  cargo check --all-targets --all-features
  cargo test
  cargo clippy --all-targets --all-features

fmt:
  -cargo shear --fix # remove all unused dependencies
  cargo fmt --all
  dprint fmt
