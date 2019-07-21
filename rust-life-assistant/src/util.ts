import * as child_process from 'child_process';
import * as vscode from 'vscode';

/** @file 
 * This file ("module") provides some helper methods.
 * Some of these, or at least the ideas for these were
 * copied from the
 * [Prusti Assistant](https://github.com/viperproject/prusti-assistant) plug-in.
 */

//let _channel: vscode.OutputChannel;
/**
 * Log function to print message. Can be changed to redirect log messages
 * to some target (e.g. a channel) later on. For now, log messages are
 * simply directly sent to the console.
 * @param message The message that shall be logged.
 */
export function log(message: string) {
	console.log(message);
	// if (!_channel) {
	// 	_channel = vscode.window.createOutputChannel("Prusti Assistant");
	// }
	// _channel.appendLine(message);
}

export interface Output {
	stdout: string;
	stderr: string;
	code: number;
}

export function spawn(
    cmd: string,
    args?: string[] | undefined,
    options?: child_process.SpawnOptions | undefined
): Promise<Output> {
	log(`Rust Life Assistant: Running '${cmd} ${args ? args.join(' ') : ''}'`);
	return new Promise((resolve, reject) => {
		let stdout = '';
		let stderr = '';

		const proc = child_process.spawn(cmd, args, options);

		proc.stdout.on('data', (data) => stdout += data);
		proc.stderr.on('data', (data) => stderr += data);
		proc.on('close', (code) => {
			log("===== Begin stdout =====");
			log(stdout);
			log("===== End stdout =====");
			log("===== Begin stderr =====");
			log(stderr);
			log("===== End stderr =====");
			resolve({ stdout, stderr, code });
		});
		proc.on('error', (err) => {
			log("===== Begin stdout =====");
			log(stdout);
			log("===== End stdout =====");
			log("===== Begin stderr =====");
			log(stderr);
			log("===== End stderr =====");
			console.log("Error", err);
			log(`Error: ${err}`);
			reject(err);
		});
	});
}
