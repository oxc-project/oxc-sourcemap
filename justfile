#!/usr/bin/env -S just --justfile

set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]
set shell := ["bash", "-cu"]

_default:
  @just --list -u

alias r := ready

init:
  cargo binstall watchexec-cli typos-cli cargo-shear dprint -y

ready:
  git diff --exit-code --quiet
  typos
  cargo check --all-targets --all-features
  cargo test
  cargo clippy --all-targets --all-features
  just fmt

fmt:
  -cargo shear --fix # remove all unused dependencies
  cargo fmt --all
  dprint fmt
