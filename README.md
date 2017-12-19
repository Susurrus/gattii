Gattii
======
[![crates.io version badge](https://img.shields.io/crates/v/gattii.svg)](https://crates.io/crates/gattii)
[![Documentation](https://docs.rs/gattii/badge.svg)](https://docs.rs/crate/gattii)


[![GitLab CI status](https://gitlab.com/susurrus/gattii/badges/master/build.svg)](https://gitlab.com/susurrus/gattii/pipelines)
[![Appveyor CI status](https://ci.appveyor.com/api/projects/status/gitlab/Susurrus/gattii?svg=true&branch=master)](https://ci.appveyor.com/project/Susurrus/gattii)
[![Travis CI status](https://travis-ci.org/Susurrus/gattii.svg?branch=master)](https://travis-ci.org/Susurrus/gattii)

Gattii is a serial terminal emulator that allows one to use a serial port for transmitting and receiving data. This project grew out of my desire to learn [Rust]. It aims to replace RealTerm_, and as such provide a very similar set of functionality. Realterm was invaluable in my robotics work with microcontrollers, but is not actively maintained, difficult to contribute to and improve, and full of bugs, both crashers and usability related.

[Rust]: https://www.rust-lang.org
[Realterm]: http://realterm.sourceforge.net/

The name Gattii refers to Cryptococcus Gattii, which is named in solidarity with Rust itself, which was named after the rust family of fungi. I forgot why I originally chose this name, but now I like it so I'm just gonna keep it.

Screenshots
===========

![Screenshot of main window](screenshot.png?raw=true)

Functionality
=============

* Enumerate ports (except on Mac)
* Easily modify settings/change ports
* Send file
* Log to file

Software Requirements
=====================

This software is written in Rust, and as such requires the Rust toolchain to be installed to build it. Additionally there are library requirements for some of supporting libraries (gtk-rs). As `gtk-rs` does not support anything older than the current Rust stable release, `gattii` is also limited to that.

The only tier-1 platform for this is Linux x64, as that's what I develop and test on. I try to be diligent at testing all functionality on Windows, but as it's not my primary OS, some things may slip through the cracks. Windows has Tier 2 support in that compilation testing is done on it, but I don't actively test functionality. Mac is Tier 3 in that compilation works, but serial enumeration isn't working until the serialport-rs 2.0 release, but do not expect this to work correctly on Mac.

Building
========

1. Install Rust stable using [rustup](https://www.rustup.rs/)
2. Install library requirements:
    * [gtk (3.14+)](http://gtk-rs.org/docs-src/requirements)
3. Clone the main repository with `git clone https://gitlab.com/susurrus/gattii.git`
4. Run `cargo build` within the project directory

Licensing
=========

This project is licensed under the GPL version 3 or later. See the LICENSE file for the license text.

If you received a compiled version of this code, a copy of the source code can be found online at https://gitlab.com/susurrus/gattii.

How to Contribute
=================

There are two ways to contribute to Gattii. The first is to file issues through the [Gitlab issue tracker](https://gitlab.com/susurrus/gattii/issues).

If you'd like to contribute code, you may submit a pull request through Gitlab:
  1. [Fork Gattii on Gitlab](https://gitlab.com/susurrus/gattii/forks/new) (you'll need an account first)
  2. Clone your fork: `git clone https://gitlab.com/YOUR_NAME/gattii.git`
  3. Install build dependencies (listed above under Building)
  4. Create commits and push to Gitlab.
  5. [Submit a merge request](https://gitlab.com/susurrus/gattii/merge_requests/new)

Attribution
===========

The send file icon is Upload by AlePio from the Noun Project.

The log to file icon is based on Download by AlePio from the Noun Project.

