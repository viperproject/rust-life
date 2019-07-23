// The module 'vscode' contains the VS Code extensibility API
// Import the module and reference it with the alias vscode in your code below
import * as vscode from 'vscode';
import * as path from 'path';
import { performance } from 'perf_hooks';
import * as util from './util';
import * as config from './config';

// this method is called when your extension is activated
// your extension is activated the very first time the command is executed
export function activate(context: vscode.ExtensionContext) {

	// Function that runs the rust-life tool on a given argument
	// Once the tool terminated, the output of it (JSON) will be opened from the file and returned for further usage,
	// after being parsed to a object.
	async function runRustLife(document: vscode.TextDocument) {
		if (document.languageId === "rust") {
			vscode.window.setStatusBarMessage("Running rust-life (compiler mod)...");
			const start = performance.now();
			const programPath = document.uri.fsPath;

			// run the tool on the document:
			const output = await util.spawn(
				//"LD_LIBRARY_PATH=" + config.rustLibPath() + " " + config.rustLifeExe(context),
				config.rustLifeExe(context),
				["--sysroot", config.rustCompilerPath(), programPath],
				{
					cwd: config.rustLifeHome(context),
					env: {
						RUST_BACKTRACE: "1",
						PATH: process.env.PATH,  // Needed e.g. to run Rustup (probably not really needed right now, but does not harm.)
						LD_LIBRARY_PATH: config.rustLibPath()
					}
				}
			);

			const duration = Math.round((performance.now() - start) / 100) / 10;
			vscode.window.setStatusBarMessage(`rust-life (compiler mod) terminated (${duration} s)`);

			let result = require(path.join(config.rustLifeHome(context), "nll-facts", "error_graph.json"));
			return result;
		} else {
			util.log(
				"The document is not a Rust program, thus rust-life (compiler mod) will not run on it."
			);
		}
	}

	// The command has been defined in the package.json file
	// Now provide the implementation of the command with registerCommand
	// The commandId parameter must match the command field in package.json
	let disposable = vscode.commands.registerCommand('extension.rustLifeVisualize', async () => {
		// The code you place here will be executed every time your command is executed

		// Display a message box to the user
		// vscode.window.showInformationMessage('Rust Life visualization started.');

		// get the name of the currently opened file and run rust life on it, getting back the error path (graph):
		let errorPath;
		if (vscode.window.activeTextEditor) {
			errorPath = await runRustLife(
				vscode.window.activeTextEditor.document
			);
		} else {
			util.log("vscode.window.activeTextEditor is not ready yet.");
		}
		if (errorPath == null) {
			vscode.window.showErrorMessage('Rust Life did not run successfully, no output available.');
			// give up, return from the command callback:
			return;
		}
		util.log(errorPath);

		vscode.window.showInformationMessage(`Currently handled function: ${errorPath.function_name}`);
	});

	context.subscriptions.push(disposable);

	// Use the console to output diagnostic information (console.log) and errors (console.error)
	// This line of code will only be executed once when your extension is activated
	console.log('Congratulations, the extension "rust-life-assistant" is now active!');
}

// this method is called when your extension is deactivated
export function deactivate() {}
