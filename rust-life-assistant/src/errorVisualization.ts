import * as vscode from 'vscode';
import * as fs from 'fs';
import * as path from 'path';
import { performance } from 'perf_hooks';
import * as util from './util';
import * as config from './config';

/**
 * @class A class that gives some basic structure and functionality for visualizing an error. (given as graph that is a
 * path, emitted from Rust Life/compiler mod as a JSON "Object".) This class is not jet functionally, but classes
 * that actually provide the full functionality for a visualization shall inherit from this class and then implement
 * everything that is needed in a somewhat uniform way. (Therefore, this class is abstract.)
 */
export abstract class Visualization {
	context: vscode.ExtensionContext;
	editor: vscode.TextEditor;
	errorPath: any;
	rustLifeOutput: util.Output | undefined;

	/**
	 * Create a new instance of a Visualization.
	 * @param context The context of the extension that this visualization is part of, needed form some interactions.
	 * @param errorPath The EnrichedErrorGraph (version only containing fields that are included in JSON dump) that
	 * shall be visualized by this instance.
	 * @param editor The text editor ("window") that the source for this error is displayed in, is needed for
	 * highlighting lines.
	 */
	constructor(context: vscode.ExtensionContext, editor: vscode.TextEditor) {
		this.context = context;
		this.editor = editor;
	}

	/**
	 * Does create the actual visualization (e.g. by calling generateHtml) and then create and set up a complete WebView
	 * that will show the visualization in the second column of the VScode window.
	 * Before, it will also run the rust-life (extract-error) executable that must be located in ~/.rust-life, to
	 * analyse the document from the editor and get the information for the visualization.
	 * @returns The created webView, for eventual further usage and treatment. (Not necessarily needed, all clean-up
	 * that is needed from the view of this method is already set up appropriately before it returns.)
	 * If something went wrong (e.g. the editor does not contain a rust file, or rust-life crashed unexpectedly), this
	 * function will return `undefined`.
	 */
	public async showPathInPanel() {
		// run rust-life (compiler mod, executable named extract-error) and set it's result (read in from the JSON, i.e.
		// the dumped form of EnrichedErrorGraph) to the global field for the errorPath (was not necessarily initialized
		// before)
		let rustLifeRes = await this.runRustLife(
			this.editor.document
		);
		if (! rustLifeRes) {
			vscode.window.showErrorMessage('Rust Life did not run successfully, no output available.\
			Is the your target rust file opened in the active tab?');
			// give up, return from the command callback:
			return;
		}
		this.errorPath = rustLifeRes.errorPath;
		this.rustLifeOutput = rustLifeRes.output;
		util.log(this.errorPath);

		// TODO check if there already is a panel for this editor (or better for this file?), only create a new one if
		// there isn't, otherwise reuse the old one that should be accessible in some way...

		// Create and show panel
		const panel = vscode.window.createWebviewPanel(
			'errorGraphView',
			`Error explanation for fn ${this.errorPath.function_name}`,
			vscode.ViewColumn.Two,
			{
				enableScripts: true,
			}
		);

		panel.webview.html = this.generateHtml();

		let onClickHandler = new OnClickHandler(this.editor);
		panel.webview.onDidReceiveMessage(onClickHandler.handleWebViewMsg, onClickHandler, this.context.subscriptions);

		let textEditorChangeDisposable = vscode.window.onDidChangeActiveTextEditor(onClickHandler.checkRestoreHighlight,
			onClickHandler);

		panel.onDidDispose(undefined => {
			textEditorChangeDisposable.dispose();
			// guess there is no need to dispose the listener onDidReceiveMessage, since is is part of the panel that is
			// disposed anyway right now.
		},
		undefined,
		this.context.subscriptions);

		return panel;
	}

	/**
	 * Get a node with only outgoing and no ingoing edges from a graph, that is given as an array of edges.
	 * (Nodes are actually regions, but for now just represented by their number.)
	 * This graph is part of the errorPath field of this Visualization instance.
	 * @returns The first node that is found. If nothing is found, (which would indicate an issue in the input, e.g. a
	 * cycle.) -1 is returned.
	 */
	protected getFirstNode(): number {
		let edges: Array<Array<number>> = this.errorPath.edges;
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
	 * Function that gets a node that is a direct successor of the one that is passed as start in the graph that is
	 * described by it's edges and it given as the filed `edges` of the errorPath field of this Visualization instance.
	 * @param start The node (it's number) that we need a successor of.
	 * @returns The number of the successor node, or -1 if none is found
	 */
	protected getNextNode(start: number): number {
		let edges: Array<Array<number>> = this.errorPath.edges;
		for (let edge of edges) {
			if (edge[0] === start) {
				return edge[1];
			}
		}
		return -1;
	}

	/**
	 * This function generates the HTML of the visualization. It will be called by showPathInPanel(...) and must be
	 * overridden by any concrete class that inherits from `Visualization`.
	 * For creating the HTML, this function shall use the `errorPath` (an EnrichedErrorPath, read from a JSON dump of
	 * Rust Life) to get the information that it shall visualize. Furthermore it may also use `rustLifeOutput`.
	 * Hence it is guaranteed (and must be guaranteed by any callee) that both `errorPath` and `rustLifeOutput` are
	 * initialized as expected. (`errorPath` to a EnrichedErrorGraph (serialization), e.g. read from JSON, and
	 * `rustLifeOutput` to an instance of util.Output. Both shall definitely not be undefined.)
	 * @returns The generates HTML as string. This shall be valid HTML that a vscode.WebViewPanel.webview.html can be
	 * set to.
	 */
	protected abstract generateHtml(): string;

	/**
	 * Function that runs the rust-life tool on a given document.
	 * Once the tool terminated, the output of it (JSON) will be opened from the file and returned for further usage,
	 * after being parsed to a object. (If nothing went terribly wrong in between it should correspond to the
	 * serialized version of the EnrichedErrorGraph struct from the used rust-life version.)
	 * @returns The EnrichedErrorGraph from the JSON and the output (giving stdout, stderr and the return code) of the
	 * Rust Life (extract-error) executable, if the execution succeeded. Otherwise, undefined is returned.
	 */
	private async runRustLife(document: vscode.TextDocument):
					Promise<{errorPath: any, output: util.Output} | undefined> {
		if (document.languageId === "rust") {
			vscode.window.setStatusBarMessage("Running rust-life (compiler mod)...");
			const start = performance.now();
			const programPath = document.uri.fsPath;

			// run the tool on the document:
			const output = await util.spawn(
				//"LD_LIBRARY_PATH=" + config.rustLibPath() + " " + config.rustLifeExe(context),
				config.rustLifeExe(this.context),
				["--sysroot", config.rustCompilerPath(), programPath],
				{
					cwd: config.rustLifeHome(this.context),
					env: {
						RUST_BACKTRACE: "1",
						PATH: process.env.PATH,  // Needed e.g. to run Rustup (probably not really needed right now, but does not harm.)
						LD_LIBRARY_PATH: config.rustLibPath()
					}
				}
			);

			const duration = Math.round((performance.now() - start) / 100) / 10;
			vscode.window.setStatusBarMessage(`rust-life (compiler mod) terminated (${duration} s)`);

			if (! output) {
				// Something with running Rust Life when (rather) wrong, we did not get an usable result.
				return undefined;
			}

			let jsonDumpPath = path.join(config.rustLifeHome(this.context), "nll-facts", "error_graph.json");
			let rawData = fs.readFileSync(jsonDumpPath, 'utf8');
			let errorPath = JSON.parse(rawData);
			return {errorPath, output};
		} else {
			util.log(
				"The document is not a Rust program, thus rust-life (compiler mod) will not run on it."
			);
			return undefined;
		}
	}

}

/**
 * @class Implements the Visualization of the error (path) as a graph.
 */
export class GraphVisualization extends Visualization {
	/**
	 * Render a EnrichedErrorGraph (e.g. read from JSON) as a complete HTML page (webview content)
	 * It will use the field Visualization.errorPath field as EnrichedErrorGraph (version only containing fields that
	 * are included in JSON dump suffices) to get the information it needs for creating the HTML.
	 * @returns The generated HTML, as string. This will be valid HTML that a vscode.WebViewPanel.webview.html can be
	 * set to.
	 */
	protected generateHtml(): string {
		let html = `<!DOCTYPE html>
		<html lang="en">
		<head>
			<meta charset="UTF-8">
			<meta name="viewport" content="width=device-width, initial-scale=1.0">
			<title>Error visualization for fn ${this.errorPath.function_name}</title>
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

		let cur_region: number = this.getFirstNode();
		util.log(`cur_region: ${cur_region}`);

		while(cur_region >= 0) {
			let local_line_nr = this.errorPath.locals_info_for_regions[cur_region][0];
			let local_name: string = this.errorPath.locals_info_for_regions[cur_region][1];
			let local_source_snip: string = this.errorPath.locals_info_for_regions[cur_region][2];
			let region_lines_str = '';
			let line_nr_from_lines_for_regions = -1;
			this.errorPath.lines_for_regions[cur_region].forEach(function (line: Array<any>) {
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
					console.warn(`Mapping to a line failed for region R${cur_region}`);
					html += `<table onclick="console.error('Mapping to a line number failed for this region, highlighting not possible!')"`;
				}
				html += `<tr><th>Lifetime R${cur_region}</th></tr>
				<tr><td>${local_name}: &amp;'R${cur_region}</td></tr>
				${region_lines_str}</table>`;
			}

			let next_region = this.getNextNode(cur_region);

			if (next_region >= 0) {
				// there is a next region, draw the constraint towards it.
				html += `<p class=arrow>↓</p>`;

				let ind = this.errorPath.lines_for_edges_start[cur_region][0];
				let point_snip = this.errorPath.lines_for_edges_start[cur_region][1];

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
}

/**
 * @class Display the error (path) as a textual representation.
 */
export class TextualVisualization extends Visualization {
	/**
	 * Render a EnrichedErrorGraph (e.g. read from JSON) as a complete HTML page (webview content)
	 * It will use the field Visualization.errorPath field as EnrichedErrorGraph (version only containing fields that
	 * are included in JSON dump suffices) to get the information it needs for creating the HTML. Furthermore, it will
	 * also use the rustLifeOutput. Hence it must be ensured that both of these fields are appropriately initialized
	 * (i.e. not undefined) before this method is called.
	 * @requires this.errorPath
	 * @requires this.rustLifeOutput
	 * @returns The generated HTML, as string. This will be valid HTML that a vscode.WebViewPanel.webview.html can be
	 * set to.
	 * However, if any precondition is violated, this method will return an empty string.
	 */
	protected generateHtml(): string {
		if ((! this.rustLifeOutput) || (! this.errorPath)) {
			return "";
		}
		let html = `<!DOCTYPE html>
		<html lang="en">
		<head>
			<meta charset="UTF-8">
			<meta name="viewport" content="width=device-width, initial-scale=1.0">
			<title>Error explanation for fn ${this.errorPath.function_name}</title>
			<style>
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

		html += `<h3>Rust compiler error (basically stderr of rustc):</h3>`;

		let rustLifeStderr = this.rustLifeOutput.stderr.replace(/\n/g, "<br>");
		html += `<p>${rustLifeStderr}</p>`;

		html += `<h3>Possible explanation for "Why is this variable still borrowed?"</h3>`;
		html += `<ol>`;

		let cur_region: number = this.getFirstNode();
		util.log(`cur_region: ${cur_region}`);

		let curRegionLocalInfo = this.getLocalInfoForRegion(cur_region);
		let local_line_nr = curRegionLocalInfo.local_line_nr;
		let local_name = curRegionLocalInfo.local_name;
		let constraint_line_nr = this.errorPath.lines_for_edges_start[cur_region][0];
		let point_snip = this.errorPath.lines_for_edges_start[cur_region][1].trim();
		html += `<li><a onclick="requestLineHighlight(${local_line_nr})">"${local_name}"</a>
		may borrow the affected variable, due to line
		<a onclick="requestLineHighlight(${constraint_line_nr})">${constraint_line_nr}: '${point_snip}'</a></li>`;

		let next_region = this.getNextNode(cur_region);

		while(next_region >= 0) {
			let curRegionLocalInfo = this.getLocalInfoForRegion(cur_region);
			let curLocalLineNr = curRegionLocalInfo.local_line_nr;
			let curLocalName = curRegionLocalInfo.local_name;
			let constraint_line_nr = this.errorPath.lines_for_edges_start[cur_region][0];
			let point_snip = this.errorPath.lines_for_edges_start[cur_region][1].trim();
			let nextRegionLocalInfo = this.getLocalInfoForRegion(next_region);
			let nextLocalLineNr = nextRegionLocalInfo.local_line_nr;
			let nextLocalName = nextRegionLocalInfo.local_name;

			html += `<li><a onclick="requestLineHighlight(${nextLocalLineNr})">"${nextLocalName}"</a> may borrow
			<a onclick="requestLineHighlight(${curLocalLineNr})">"${curLocalName}"</a>, due to line
			<a onclick="requestLineHighlight(${constraint_line_nr})">${constraint_line_nr}: '${point_snip}'</a></li>`;


			cur_region = next_region;
			next_region = this.getNextNode(cur_region);

			util.log(`cur_region: ${cur_region}`);

		}

		let lastRegionLocalInfo = this.getLocalInfoForRegion(cur_region);
		let lastLocalLineNr = lastRegionLocalInfo.local_line_nr;
		let lastLocalName = lastRegionLocalInfo.local_name;

		html += `<li><a onclick="requestLineHighlight(${lastLocalLineNr})">"${lastLocalName}"</a> is later used</li>`;

		html += `</ol></body>`;
		return html;
	}

	/**
	 * Get information about the local that is associated with a region. (More exactly, the line number it is defined
	 * on and it's name) First, it tries to get the information from errorPath.locals_info_for_regions. If it does not
	 * find good information there (e.g. if the line number entry is smaller then 1), it will try to find information in
	 * errorPath.lines_for_regions, whereof it will take the line number from the first entry (if there is any), and
	 * then try to parse the source code line to get the name of the local.
	 * @param region The region for which the local information shall be acquired.
	 * @returns It will return an object that contains a field local_line_nr that gives the line number where the local
	 * is defined (Indexed from 1, i.e. like counting lines in a text editor), and a field local_name that gives tha
	 * name of the local, as a string. If it fails to get the line number, it will return a value of 0 or lower and an
	 * empty string as local_name. (If only getting the number succeeds, the string will also be empty.)
	 */
	private getLocalInfoForRegion(region: number): {local_line_nr: number, local_name: string} {
		let local_line_nr = this.errorPath.locals_info_for_regions[region][0];
		let local_name = this.errorPath.locals_info_for_regions[region][1];

		if (local_line_nr < 1) {
			// No local was found for this region, try to get the information (line number and local name) for the
			// lines_for_regions information.
			if (this.errorPath.lines_for_regions[region] && this.errorPath.lines_for_regions[region].length > 0) {
				// if there are lines for regions, simply take the first of them and use it's information.
				local_line_nr = this.errorPath.lines_for_regions[region][0][0];
				let local_line_str: string = this.errorPath.lines_for_regions[region][0][1];
				let letNameRegEx = /let[\s]+[\w]+/;
				let letLocalMatches = local_line_str.match(letNameRegEx);
				if (letLocalMatches) {
					// matching succeeded, get the local name (otherwise, it will remain to be an empty string.)
					let letLocalMatchesSplits = letLocalMatches[0].split(/\s+/);
					if(! letLocalMatchesSplits[1].includes("mut")) {
						// the matched second element is the name, set it
						local_name = letLocalMatchesSplits[1];
					} else {
						// the matched element is the `mut` keyword, retry to match as a `let mut {localName}` statement.
						let letMutNameRegEx = /let[\s]+mut[\s]+[\w]+/;
						let letMutLocalMatches = local_line_str.match(letMutNameRegEx);
						if (letMutLocalMatches) {
							// matched as `let mut {localName}` statement, the element tow will be the local name
							local_name = letMutLocalMatches[0].split(/\s+/)[2];
						}
					}
				}
			}
		}

		return {local_line_nr, local_name};
	}
}

/**
 * @class A class that provides functionalities to handel onClick interaction (passed as messages) from a WebView panel
 * and also to apply highlighting of lines (of source code) based on these interactions in an associated text editor.
 * (Conceptually, this is a tab in VScode that shows source code, in our case more specifically it shows Rust code.)
 */
export class OnClickHandler {
	editor: vscode.TextEditor;
	fileUri: vscode.Uri;
	curLineNr: number;

	yellowBgDecoration = vscode.window.createTextEditorDecorationType({
		backgroundColor: 'yellow',
	});

	/**
	 * Constructor, setting the error path (must be the version that was dumped to JSON by rust-life/compiler mod)
	 * @param editor The editor that this handler is operating on, mainly when applying highlighting to lines.
	 * (This will be eventually updated based on the editor's document's Uri if the editor is closed/disposed.)
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