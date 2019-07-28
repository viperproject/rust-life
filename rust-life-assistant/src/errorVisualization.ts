import * as vscode from 'vscode';
import * as util from './util';

/**
 * @class A class that gives some basic structure and functionality for visualizing an error. (given as graph that is a
 * path, emitted from Rust Life/compiler mod as a JSON "Object".) This class is not jet functionally, but classes
 * that actually provide the full functionality for a visualization shall inherit from this class and then implement
 * everything that is needed in a somewhat uniform way. (Therefore, this class is abstract.)
 */
export abstract class Visualization {
	context: vscode.ExtensionContext;
	errorPath: any;
	editor: vscode.TextEditor;

	/**
	 * Create a new instance of a Visualization.
	 * @param context The context of the extension that this visualization is part of, needed form some interactions.
	 * @param errorPath The EnrichedErrorGraph (version only containing fields that are included in JSON dump) that
	 * shall be visualized by this instance.
	 * @param editor The text editor ("window") that the source for this error is displayed in, is needed for
	 * highlighting lines.
	 */
	constructor(context: vscode.ExtensionContext, errorPath: any, editor: vscode.TextEditor) {
		this.context = context;
		this.errorPath = errorPath;
		this.editor = editor;
	}

	/**
	 * Does create the actual visualization (e.g. by calling generateHtml) and then create and set up a complete WebView
	 * that will show the visualization in the second column of the VScode window.
	 * @returns The created webView, for eventual further usage and treatment. (Not necessarily needed, all clean-up
	 * that is needed from the view of this method is already set up appropriately before it returns.)
	 */
	public showPathInPanel(errorPath: any, editor: vscode.TextEditor) {
		// TODO check if there already is a panel for this editor (or better for this file?), only create a new one if
		// there isn't, otherwise reuse the old one that should be accessible in some way...

		// Create and show panel
		const panel = vscode.window.createWebviewPanel(
			'errorGraphView',
			`Error Visualization for fn ${errorPath.function_name}`,
			vscode.ViewColumn.Two,
			{
				enableScripts: true,
			}
		);

		panel.webview.html = this.generateHtml();

		let onClickHandler = new OnClickHandler(editor);
		panel.webview.onDidReceiveMessage(onClickHandler.handleWebViewMsg, onClickHandler, this.context.subscriptions);

		let textEditorChangeDisposable = vscode.window.onDidChangeActiveTextEditor(onClickHandler.checkRestoreHighlight,
			onClickHandler);

		let documentUri = editor.document.uri;
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
	 * Rust Life) to get the information that it shall visualize.
	 * @returns The generates HTML as string. This shall be valid HTML that a vscode.WebViewPanel.webview.html can be
	 * set to.
	 */
	protected abstract generateHtml(): string;
}

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
			<title>Error Visualization for fn ${this.errorPath.function_name}</title>
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
					html += `<table onclick="alert(\\"Mapping to a line number failed for this region, highlighting not possible!\\")"`;
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