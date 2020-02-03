// © 2020, ETH Zurich
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

// The module 'vscode' contains the VS Code extensibility API
// Import the module and reference it with the alias vscode in your code below
import * as vscode from 'vscode';
import * as util from './util';
import * as errorVisualization from './errorVisualization';

// this method is called when your extension is activated
// your extension is activated the very first time the command is executed
export function activate(context: vscode.ExtensionContext) {

	let disposableVisualizeGraph = vscode.commands.registerCommand('extension.rustLifeVisualizeGraph', async () => {
		// The code you place here will be executed every time your command is executed

		// Display a message box to the user
		// vscode.window.showInformationMessage('Rust Life visualization started.');

		if (vscode.window.activeTextEditor) {
			// get the currently visible editor and request a visualization for the file it shows
			let editor = vscode.window.activeTextEditor;
			const graphVisualization = new errorVisualization.GraphVisualization(context, editor);
			const visualizationPanel = graphVisualization.showPathInPanel();
		} else {
			util.log("vscode.window.activeTextEditor is not ready yet.");
		}
	});
	context.subscriptions.push(disposableVisualizeGraph);

	let disposableVisualizeTextual = vscode.commands.registerCommand('extension.rustLifeVisualizeTextual', async () => {
		if (vscode.window.activeTextEditor) {
			// get the currently visible editor and request a explanation for the file it shows
			let editor = vscode.window.activeTextEditor;
			const graphVisualization = new errorVisualization.TextualVisualization(context, editor);
			const visualizationPanel = graphVisualization.showPathInPanel();
		} else {
			util.log("vscode.window.activeTextEditor is not ready yet.");
		}
	});
	context.subscriptions.push(disposableVisualizeTextual);

	// Use the console to output diagnostic information (console.log) and errors (console.error)
	// This line of code will only be executed once when your extension is activated
	console.log('Congratulations, the extension "rust-life-assistant" is now active!');
}

// this method is called when your extension is deactivated
// TODO delete generated output file (for now in ~/.rust-life) in this case would probably be nice.
export function deactivate() {}
