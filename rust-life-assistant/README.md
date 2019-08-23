# Rust Life Assistant

This is an IDE extensions that does provides simple visualizations for complex lifetime errors. For doing so it does use Rust Life.

## Features
This extension can both show the graph-based (more precisely, path-based) visualization of an error that is created by Rust Life, as well as create a simple step-by-step guide in text form.

Both outputs are interactive, this means that parts of them (either the nodes of the graph or the blue parts of the text) can be clicked to request a highlighting of a line of source code.

The graph-based visualization is requested by the command `Rust Life: Visualize as Graph`, and the text based by `Rust Life: Explain with Text` (Hit `Ctrl+Shift+P` to open the command plate.) Please note that Rust Life might need some time to complete its execution, please be patient.

<!-- TODO: provide a better description -->
<!-- Describe specific features of your extension including screenshots of your extension in action. Image paths are relative to this README file.

For example if there is an image subfolder under your extension project workspace:

\!\[feature X\]\(images/feature-x.png\)

> Tip: Many popular extensions utilize animations. This is an excellent way to show off your extension! We recommend short, focused animations that are easy to follow. -->

## Requirements

This extension will only work on a system that is set up appropriately, since it has rather strict requirements. (It is not that flexible, esp. since some paths are hardcoded for now.)

Since no pre-built versions are available, one needs a working development environment for VS code extensions. This is best achieved by following the instructions in [Your First Extension](https://code.visualstudio.com/api/get-started/your-first-extension).

In addition, Rust must be installed, since a specific nightly version is required we strongly recommend using rustup. (Pleas stick to default settings regarding paths for storing the files of rustup.)

Then, follow these steps to make everything ready for using Rust Life Assistant:
- First you need to get a copy of the Rust Life executable. Currently, Rust Life Version 0.3.0 is required. Build it with by the following steps:
    - cd to the `compiler_mod` directory.
    - run the command `make build` to start the build process. This might take some time.
    - Note that by doing so rustup will also install the `nightly-2019-05-21` toolchain, which is must be installed to use Rust Life (Assistant).
      This will need approximately 1 GB of disk space.
- The generated executable is located in `compiler_mod/target/debug` and called `extract-error`
- Copy this executable into a folder named `.rust-life` in your home directory. (More precisely, in the home directory of the user that shall use Rust Life).
  Do not alter the name of the executable.
- Now open the extension folder with the extension (`rust-life-assistant`) in VS code and hit F5 to build the extension and run it.
- The extension development host (essentially another VS code instance) will start, and Rust Life Assistant is available for being used in it.

Note: Due to the hardcoded file system paths this extension can only be used on GNU/Linux systems. It was only tested on an Ubunut-based platform, but will most likely also work on any other distributions that can run VS code and rustup. (However, it will most likely not work on Windows or macOS)

<!-- ## Extension Settings

Include if your extension adds any VS Code settings through the `contributes.configuration` extension point.

For example:

This extension contributes the following settings:

* `myExtension.enable`: enable/disable this extension
* `myExtension.thing`: set to `blah` to do something -->

## Known Issues

- If Rust Life fails, Rust Life Assistant might display old results from a
  previous execution. This is a simple implementation insufficiency that
  could be resolved by a simple clean-up step.
- In some cases the parsing of source lines to get local variable's names will fail and include something that is not the name of a local variable.
- Right now, there is not an official option to deactivate source code highlighting. Once it was triggered, one line will always stay highlighted. It can be deactivated by first closing the visualization that created it, then switching to a different tab (rendering the affected one invisible) and then switching back to it.
- Rust Life contains hardcoded paths to files, that probably prevent it from ruing on different platforms then GNU/Linux. Mitigating this would probably be doable, but might take some time.
- Some of the security guidelines for extensions, esp. for WebViews are currently violated. This should be fixed before using this extension in production, esp. if it will eventually include using online content in the future.
- There probably are quite some more issues in the code.

Note: This list only provides issues specific to the implementation of Rust Life Assistant, some issues that mostly affect Rust Life are given in the thesis.

<!-- ## Release Notes

Users appreciate release notes as you update your extension.

### 1.0.0

Initial release of ...

### 1.0.1

Fixed issue #.

### 1.1.0

Added features X, Y, and Z. -->

<!-- -----------------------------------------------------------------------------------------------------------

## Working with Markdown

**Note:** You can author your README using Visual Studio Code.  Here are some useful editor keyboard shortcuts:

* Split the editor (`Cmd+\` on macOS or `Ctrl+\` on Windows and Linux)
* Toggle preview (`Shift+CMD+V` on macOS or `Shift+Ctrl+V` on Windows and Linux)
* Press `Ctrl+Space` (Windows, Linux) or `Cmd+Space` (macOS) to see a list of Markdown snippets

### For more information

* [Visual Studio Code's Markdown Support](http://code.visualstudio.com/docs/languages/markdown)
* [Markdown Syntax Reference](https://help.github.com/articles/markdown-basics/)

**Enjoy!** -->