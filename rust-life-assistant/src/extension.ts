// The module 'vscode' contains the VS Code extensibility API
// Import the module and reference it with the alias vscode in your code below
import * as vscode from 'vscode';
import * as path from 'path';
import * as fs from 'fs';
import { performance } from 'perf_hooks';
import * as util from './util';
import * as config from './config';
import { on } from 'cluster';

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

			// This seems not to load the JXOn again on a second execution, leads to mess, try to read JSON differently.
			// let result = require(path.join(config.rustLifeHome(context), "nll-facts", "error_graph.json"));
			let jsonDumpPath = path.join(config.rustLifeHome(context), "nll-facts", "error_graph.json");
			let rawData = fs.readFileSync(jsonDumpPath, 'utf8');
			let result = JSON.parse(rawData);
			return result;
		} else {
			util.log(
				"The document is not a Rust program, thus rust-life (compiler mod) will not run on it."
			);
		}
	}

	class OnClickHandler {
		editor: vscode.TextEditor;
		fileUri: vscode.Uri;
		curLineNr: number;

		yellowBgDecoration = vscode.window.createTextEditorDecorationType({
			backgroundColor: 'yellow',
		});

		/**
		 * Constructor, setting the error path (must be the version that was dumped to JSON by rust-life/compiler mod)
		 * @param ep The error path structure.
		 */
		constructor(editor: vscode.TextEditor) {
			this.editor =  editor;
			this.fileUri = editor.document.uri;
			this.curLineNr = -2;
		}

		/**
		 * Highlight a line, in yellow colour, by using yellowBgDecoration and setting the range for this line as the
		 * one that shall be highlighted. Note that this will overwritten, and hence remove any previous highlighting.
		 * The highlighting is done on the editor that is given as field of this class instance.
		 * @param lineNr the number of the line, as line number when counting (indexing) starting from 1. (As in the
		 * EnrichedErrorGraph JSON dump.) It must be strictly bigger then 0, and never strictly bigger then the number of
		 * lines of the document of this.editor. (If such an invalid value is passed, the method will do nothing, and
		 * report this to the log.)
		 */
		highlightLine(lineNr: number) {
			if (lineNr <= 0 || lineNr > this.editor.document.lineCount) {
				util.log(`Cannot highligh line ${lineNr}, this is not a valid line number in the current source.`);
				return;
			}
			this.curLineNr = lineNr;
			let line = lineNr - 1;
			let lastCharIndex =  this.editor.document.lineAt(line).text.length;
			let range = new vscode.Range(line, 0, line, lastCharIndex);
			let decorationsArray: vscode.DecorationOptions[] = [];
			decorationsArray.push({ range });
			this.editor.setDecorations(this.yellowBgDecoration, decorationsArray);
		}

		/**
		 * Function that is registered as callback when a message from the webView arrives. (Hence, this method is called
		 * in this case and will deal with incoming messages.)
		 * @param message The message that was passed.
		 */
		public handleWebViewMsg(message: any) {
			util.log(`Received message from WebView:`);
			util.log(message);

			switch(message.command) {
				case('highlight_line'): this.highlightLine(message.lineNr); break;
				default: console.error("Received a unknown command from the WebView, this is most likely a bug!");
			}
		}

		/**
		 * This function must be called when the active text editor changed (back to the one that shall contain the
		 * highlighting, or that shows the file that the highlighting is for.)
		 * This will check if there is an active highlight for this callback (curLineNr is bigger then -2), and if the
		 * editor that is passed is showing the file that this highlight is for. If so, it will re-apply the
		 * highlighting to the editor.
		 * In this case, the function will also update the editor field of the class instance to point to the new
		 * editor, to allow future highlight requests to work.
		 * Note that this function also works when called with an undefined argument, but will do nothing in this case.
		 * This is to be directly registrable as callback for window.onDidChangeActiveTextEditor
		 */
		public checkRestoreHighlight(newEditor: vscode.TextEditor | undefined) {
			if (newEditor && this.curLineNr > -2 && newEditor.document.uri === this.fileUri) {
				this.editor = newEditor;
				this.highlightLine(this.curLineNr);
			}
		}
	}

	/**
	 * Takes an EnrichedErrorGraph structure (e.g. read from JSON dump from rust-life compiler mod) and displays it in
	 * a newly created webView. (Note that these graphs actually are only a path.)
	 * @param errorPath The EnrichedErrorGraph (version only containing fields that are included in JSON dump)
	 * @param editor The text editor ("window") that the source for this error is displayed in, is needed for
	 * highlighting lines.
	 * @returns The created webView, for eventual further usage and treatment.
	 */
	function showPathInPanel(errorPath: any, editor: vscode.TextEditor) {
		// TODO check if there already is a panel, only create a new one if there isn't, otherwise reuse old one.
		// Create and show panel
		const panel = vscode.window.createWebviewPanel(
			'errorGraphView',
			`Error Visualization for fn ${errorPath.function_name}`,
			vscode.ViewColumn.Two,
			{
				enableScripts: true,
			}
		);

		panel.webview.html = generateHtml(errorPath);

		let onClickHandler = new OnClickHandler(editor);
		panel.webview.onDidReceiveMessage(onClickHandler.handleWebViewMsg, onClickHandler, context.subscriptions);

		let textEditorChangeDisposable = vscode.window.onDidChangeActiveTextEditor(onClickHandler.checkRestoreHighlight,
			onClickHandler);

		let documentUri = editor.document.uri;
		panel.onDidDispose(undefined => {
			textEditorChangeDisposable.dispose();
			// guess there is no need to dispose the listener onDidReceiveMessage, since is is part of the panel that is
			// disposed anyway right now.
		},
		undefined,
		context.subscriptions);

		return panel;
	}

	/**
	 * Render a EnrichedErrorGraph (e.g. read from JSON) as a complete HTML page (webview content) 
	 * @param errorPath The EnrichedErrorGraph (version only containing fields that are included in JSON dump suffices)
	 * @returns The generated HTML, as string
	 */
	function generateHtml(errorPath: any): string {
		let html = `<!DOCTYPE html>
		<html lang="en">
		<head>
			<meta charset="UTF-8">
			<meta name="viewport" content="width=device-width, initial-scale=1.0">
			<title>Error Visualization for fn ${errorPath.function_name}</title>
			<style>
			table {
				border-collapse: collapse;
			}
			table, th, td {
				display: flex;
				align-items: center;
				justify-content: center;
			}
			th, td {
				border: 1px solid;
			}
			.arrow {
				display: flex;
				align-items: center;
				justify-content: center;

				padding: 0px;
				margin: 0px;
				font-size: 40px;
				font-weight: bold;

			}
			</style>
			<script>
			// TODO this might not be too good style, but it does work.
			const vscode = acquireVsCodeApi();
			/**
			 * This function does pass a message back to the IDE extension (that owns this WebView) to request
			 * highlighting a certain line in the text editor.
			 * @param lineNr The number of the line that shall be highlighted, indexed from 1, i.e. like when counting
			 * line numbers in an editor window.
			 */
			function requestLineHighlight(lineNr) {
				console.log(\`User requested a highlight of line \${lineNr}\`);
				vscode.postMessage({
					command: 'highlight_line',
					lineNr: \`\${lineNr}\`,
				})
			}
			</script>
		</head>
		<body>`;

		let cur_region: number = getFirstNode(errorPath.edges);
		util.log(`cur_region: ${cur_region}`);

		while(cur_region >= 0) {
			let local_line_nr = errorPath.locals_info_for_regions[cur_region][0];
			let local_name: string = errorPath.locals_info_for_regions[cur_region][1];
			let local_source_snip: string = errorPath.locals_info_for_regions[cur_region][2];
			let region_lines_str = '';
			let line_nr_from_lines_for_regions = -1;
			errorPath.lines_for_regions[cur_region].forEach(function (line: Array<any>) {
				if (line_nr_from_lines_for_regions < 0) {
					line_nr_from_lines_for_regions = line[0];
				}
				region_lines_str += `<tr><td>${line[0]}: ${line[1].trim()}</td></tr>`;
			});

			if (local_source_snip.length > 0) {
				html += `<table onclick="requestLineHighlight(${local_line_nr})">
				<tr><th>Lifetime R${cur_region}</th></tr>
				<tr><td>${local_name}: &amp;'R${cur_region}</td></tr>
				<tr><td>${local_line_nr}: ${local_source_snip}</td></tr>
				${region_lines_str}</table>`;
			} else {
				if (local_line_nr >= 1) {
					html += `<table onclick="requestLineHighlight(${local_line_nr})">`;
				} else if (line_nr_from_lines_for_regions >= 0) {
					html += `<table onclick="requestLineHighlight(${line_nr_from_lines_for_regions})"`;
				} else {
					html += `<table onclick="alert(\\"Mapping to a line number failed for this region, highlighting not possible!\\")"`;
				}
				html += `<tr><th>Lifetime R${cur_region}</th></tr>
				<tr><td>${local_name}: &amp;'R${cur_region}</td></tr>
				${region_lines_str}</table>`;
			}

			let next_region = getNextNode(errorPath.edges, cur_region);

			if (next_region >= 0) {
				// there is a next region, draw the constraint towards it.
				html += `<p class=arrow>↓</p>`;

				let ind = errorPath.lines_for_edges_start[cur_region][0];
				let point_snip = errorPath.lines_for_edges_start[cur_region][1];

				html += `<table onclick="requestLineHighlight(${ind})">
				<tr><th>Constraint</th></tr>
				<tr><td>R${next_region} may point to R${cur_region}</td></tr>
				<tr><td> generated at line ${ind}:</td></tr>
				<tr><td>${point_snip.trim()}</td></tr></table>`;

				html += `<p class=arrow>↓</p>`;
			}


			cur_region = next_region;
			util.log(`cur_region: ${cur_region}`);

		}

		html += `</body>`;
		return html;
	}

	/**
	 * Get a node with only outgoing and no ingoing edges from a graph, that is given as an array of edges.
	 * (Nodes are actually regions, but for now just represented by their number.)
	 * @param edges The edges of the graph, given as array or arrays (that are actually tuples with size two) of numbers
	 * @returns The first node that is found. If nothing is found, (which would indicate an issue in the input, e.g. a
	 * cycle.) -1 is returned.
	 */
	function getFirstNode(edges: Array<Array<number>>): number {
		for (let edge of edges) {
			if (edges.findIndex(inner_edge => {
				return edge[0] === inner_edge[1];
			}) < 0) {
				return edge[0];
			}
		}
		return -1;
	}

	/**
	 * Function that takes a graph, as set of edges, and gets a node that is a direct successor of the one that is
	 * passed as start.
	 * @param edges The graph, given as set of edges, with (unique) positive numbers (integers) as nodes.
	 * @param start The node (it's number) that we need a successor of.
	 * @returns The number of the successor node, or -1 if none is found
	 */
	function getNextNode(edges: Array<Array<number>>, start: number): number {
		for (let edge of edges) {
			if (edge[0] === start) {
				return edge[1];
			}
		}
		return -1;
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
			let editor = vscode.window.activeTextEditor;
			errorPath = await runRustLife(
				editor.document
			);
			if (errorPath == null) {
				vscode.window.showErrorMessage('Rust Life did not run successfully, no output available.\
				Is the your target rust file opened in the active tab?');
				// give up, return from the command callback:
				return;
			}
			util.log(errorPath);

			vscode.window.showInformationMessage(`Currently handled function: ${errorPath.function_name}`);

			const visualizationPanel = showPathInPanel(errorPath, editor);
		} else {
			util.log("vscode.window.activeTextEditor is not ready yet.");
		}
	});

	context.subscriptions.push(disposable);

	// Use the console to output diagnostic information (console.log) and errors (console.error)
	// This line of code will only be executed once when your extension is activated
	console.log('Congratulations, the extension "rust-life-assistant" is now active!');
}

// this method is called when your extension is deactivated
export function deactivate() {}
