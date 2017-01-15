Gattii
======

|crate badge| |docs badge|

.. |crate badge| image:: https://img.shields.io/crates/v/gattii.svg
                 :target: https://crates.io/crates/gattii
.. |docs badge| image:: https://docs.rs/gattii/badge.svg
                :target: https://docs.rs/crate/gattii

Gattii is a serial terminal emulator that allows one to use a serial port for transmitting and receiving data. This project grew out of my desire to learn Rust_. It aims to replace RealTerm_, and as such provide a very similar set of functionality. Realterm was invaluable in my robotics work with microcontrollers, but is not actively maintained, difficult to contribute to and improve, and full of bugs, both crashers and usability related.

.. _Rust: https://www.rust-lang.org
.. _Realterm: http://realterm.sourceforge.net/

The name Gattii refers to Cryptococcus Gattii, which is named in solidarity with Rust itself, which was named after the rust family of fungi. I forgot why I originally chose this name, but now I like it so I'm just gonna keep it.

Screenshots
===========

.. image:: resources/screenshot.png?raw=true
   :alt Screenshot of main window

Functionality
=============

Gattii is currently **BETA-LEVEL SOFTWARE**! This is as much a learning experience for me as it is a productive piece of software. This project is under active and frequent development. There are many known and unknown problems as well as missing functionality. Additionally this project pushes the limits of several packages (gtk-rs and serialport-rs especially) and as such development may be slow at times.

That being said, the current functionality is sufficient for basic ASCII/UTF-8 interaction with the serial port. Opening and changing ports and baud rates, reading and writing textual data, and sending files is all possible.

Software Requirements
=====================

This software is written in Rust, and as such requires the Rust toolchain to be installed to build it. Additionally there are library requirements for some of supporting libraries (gtk-rs).

The only tier-1 platform for this is Linux x64, as that's what I develop and test on. I try to be diligent at testing all functionality on Windows, but as it's not my primary OS, some things may slip through the cracks. Windows has Tier 2 support in that compilation testing is done on it, but I don't actively test functionality. That being said, pretty much all functionality should be cross-platform as platform-specific features are under the serialport-rs library.

Building
========

1. Install Rust stable using `rustup <https://www.rustup.rs/>`_
2. Install library requirements:
    * `gtk <http://gtk-rs.org/docs-src/requirements>`_
3. Clone the main repository with ``git clone https://gitlab.com/susurrus/gattii.git``
4. Run ``cargo build`` within the project directory

Licensing
=========

This project is licensed under the GPL version 3 or later. See the LICENSE file for the license text.

How to Contribute
=================

There are two ways to contribute to Gattii. The first is to file issues through the `Gitlab issue tracker <https://gitlab.com/susurrus/gattii/issues>`_.

If you'd like to contribute code, you may submit a pull request through Gitlab:
  1. `Fork Gattii on Gitlab <https://gitlab.com/susurrus/gattii/forks/new>`_ (you'll need an account first)
  2. Clone your fork: ``git clone https://gitlab.com/YOUR_NAME/gattii.git``
  3. Install build dependencies (listed above under Building)
  4. Create commits and push to Gitlab.
  5. `Submit a merge request <https://gitlab.com/susurrus/gattii/merge_requests/new>`_
