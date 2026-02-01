import * as vscode from 'vscode';
import * as path from 'path';

export interface ParsedTask {
	isCompleted: boolean;
	text: string;
	line: number;
	log: string;
}

export function parseTasks(lines: string[], targetDate: string): ParsedTask[] {
	const tasks: ParsedTask[] = [];
	let currentTask: { indent: number; completed: boolean; text: string; line: number } | null = null;

	for (let i = 0; i < lines.length; i++) {
		const text = lines[i];

		const taskMatch = text.match(/^(\s*)-\s*\[([ x])\]\s*(.*)/);
		if (taskMatch) {
			currentTask = {
				indent: taskMatch[1].length,
				completed: taskMatch[2] === 'x',
				text: taskMatch[3],
				line: i
			};
			continue;
		}

		const dateMatch = text.match(/^(\s*)-\s*(\d{4}-\d{2}-\d{2}):\s*(.*)/);
		if (dateMatch && currentTask) {
			const dateIndent = dateMatch[1].length;
			const dateStr = dateMatch[2];
			const logContent = dateMatch[3];

			if (dateStr === targetDate && dateIndent > currentTask.indent) {
				tasks.push({
					isCompleted: currentTask.completed,
					text: currentTask.text,
					line: currentTask.line,
					log: logContent
				});
			}
		}
	}
	return tasks;
}

export function activate(context: vscode.ExtensionContext) {

	let currentPanel: vscode.WebviewPanel | undefined;

	const disposable = vscode.commands.registerCommand('daily-task-logger.showToday', async () => {
		const todayStr = getLocalDateString();

		if (currentPanel) {
			// 既存パネルがあれば再利用
			currentPanel.reveal(vscode.ViewColumn.Beside);
		} else {
			currentPanel = vscode.window.createWebviewPanel(
				'dailyTasks',
				`今日のタスク (${todayStr})`,
				vscode.ViewColumn.Beside,
				{ enableScripts: true }
			);
			currentPanel.onDidDispose(() => {
				currentPanel = undefined;
			}, null, context.subscriptions);

			// Webview からのメッセージを受け取りファイルを開く
			currentPanel.webview.onDidReceiveMessage(async (message: { command: string; fileUri: string; line: number }) => {
				if (message.command === 'openFile') {
					const uri = vscode.Uri.parse(message.fileUri);
					const doc = await vscode.workspace.openTextDocument(uri);
					await vscode.window.showTextDocument(doc, {
						selection: new vscode.Range(message.line, 0, message.line, 0),
						viewColumn: vscode.ViewColumn.One
					});
				}
			}, null, context.subscriptions);
		}

		currentPanel.webview.html = await buildHtml(todayStr);
	});

	context.subscriptions.push(disposable);
}

// ローカルタイムゾーンで YYYY-MM-DD を取得する関数
function getLocalDateString(): string {
	const d = new Date();
	const year = d.getFullYear();
	const month = String(d.getMonth() + 1).padStart(2, '0');
	const day = String(d.getDate()).padStart(2, '0');
	return `${year}-${month}-${day}`;
}

interface FileTaskGroup {
	fileName: string;
	tasks: Array<{ isCompleted: boolean; text: string; fileUri: string; line: number; log: string }>;
}

async function collectTasks(targetDate: string): Promise<FileTaskGroup[]> {
	const workspaceFiles = await vscode.workspace.findFiles('**/*.md', '**/node_modules/**');

	// ワークスペース内のファイル + 開いている .md ファイルを合算し、URI で重複排除
	const seen = new Set<string>();
	const allFileUris: vscode.Uri[] = [];
	for (const uri of workspaceFiles) {
		const key = uri.toString();
		if (!seen.has(key)) {
			seen.add(key);
			allFileUris.push(uri);
		}
	}
	for (const doc of vscode.workspace.textDocuments) {
		if (doc.uri.scheme === 'file' && doc.languageId === 'markdown') {
			const key = doc.uri.toString();
			if (!seen.has(key)) {
				seen.add(key);
				allFileUris.push(doc.uri);
			}
		}
	}

	const groups: FileTaskGroup[] = [];

	for (const fileUri of allFileUris) {
		const doc = await vscode.workspace.openTextDocument(fileUri);
		const lines: string[] = [];
		for (let i = 0; i < doc.lineCount; i++) {
			lines.push(doc.lineAt(i).text);
		}
		const tasksInFile = parseTasks(lines, targetDate);

		if (tasksInFile.length > 0) {
			const relativePath = vscode.workspace.asRelativePath(fileUri);
			groups.push({
				fileName: path.basename(relativePath),
				tasks: tasksInFile.map(t => ({
					isCompleted: t.isCompleted,
					text: t.text,
					fileUri: fileUri.toString(),
					line: t.line,
					log: t.log
				}))
			});
		}
	}

	return groups;
}

function escapeHtml(text: string): string {
	return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

async function buildHtml(todayStr: string): Promise<string> {
	const groups = await collectTasks(todayStr);

	let body = '';
	if (groups.length === 0) {
		body = `
			<p>今日のタスク（ログ行: ${escapeHtml(todayStr)}）は見つかりませんでした。</p>
			<p>タスクの下に &quot;- ${escapeHtml(todayStr)}: ログ&quot; を追加してみてください。</p>
			<p>※Markdownファイルが保存されているかも確認してください。</p>`;
	} else {
		for (const group of groups) {
			body += `<h2>${escapeHtml(group.fileName)}</h2>\n<ul>\n`;
			for (const task of group.tasks) {
				const checkbox = task.isCompleted ? '&#9745;' : '&#9744;';
				const dataAttr = `data-uri="${escapeHtml(task.fileUri)}" data-line="${task.line}"`;
				body += `<li>${checkbox} <a href="#" class="task-link" ${dataAttr}>${escapeHtml(task.text)}</a>\n`;
				body += `  <br><span class="log">📝 ${escapeHtml(task.log)}</span></li>\n`;
			}
			body += `</ul>\n`;
		}
	}

	return `<!DOCTYPE html>
<html lang="ja">
<head>
<meta charset="UTF-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline';">
<style>
	body { font-family: var(--vscode-font-family); color: var(--vscode-foreground); padding: 12px; }
	h1 { font-size: 1.4em; }
	h2 { font-size: 1.1em; margin-top: 1.2em; }
	ul { list-style: none; padding-left: 0; }
	li { margin-bottom: 8px; }
	.task-link { color: var(--vscode-textLink-foreground); cursor: pointer; text-decoration: underline; }
	.task-link:hover { color: var(--vscode-textLink-activeForeground); }
	.log { color: var(--vscode-descriptionForeground); margin-left: 24px; }
</style>
</head>
<body>
<h1>今日のタスク一覧 (${escapeHtml(todayStr)})</h1>
${body}
<script>
	const vscode = acquireVsCodeApi();
	document.addEventListener('click', (e) => {
		const link = e.target.closest('.task-link');
		if (link) {
			e.preventDefault();
			vscode.postMessage({
				command: 'openFile',
				fileUri: link.dataset.uri,
				line: Number(link.dataset.line)
			});
		}
	});
</script>
</body>
</html>`;
}
