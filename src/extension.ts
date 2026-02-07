import * as vscode from 'vscode';
import * as path from 'path';
import * as os from 'os';
import { TaskTreeProvider } from './taskTreeProvider';

export interface ParsedTask {
	isCompleted: boolean;
	text: string;
	line: number;
	log: string;
}

export interface ParsedTaskWithDate extends ParsedTask {
	date: string;
}

import { parseTasks, parseTasksAllDates } from './parser';
export { parseTasks, parseTasksAllDates };

export function activate(context: vscode.ExtensionContext) {

	// TreeViewの登録
	const taskTreeProvider = new TaskTreeProvider();
	const treeView = vscode.window.createTreeView('taskiView', {
		treeDataProvider: taskTreeProvider,
		showCollapseAll: true
	});
	context.subscriptions.push(treeView);

	// showTodayコマンド: TreeViewを表示
	const showTodayDisposable = vscode.commands.registerCommand('taski.showToday', async () => {
		await vscode.commands.executeCommand('taskiView.focus');
		taskTreeProvider.refresh();
	});
	context.subscriptions.push(showTodayDisposable);

	// リフレッシュコマンド
	const refreshDisposable = vscode.commands.registerCommand('taski.refreshTasks', () => {
		taskTreeProvider.refresh();
	});
	context.subscriptions.push(refreshDisposable);

	// タスクの場所を開くコマンド
	const openTaskLocationDisposable = vscode.commands.registerCommand('taski.openTaskLocation', async (fileUri: string, line: number) => {
		const uri = vscode.Uri.parse(fileUri);
		const doc = await vscode.workspace.openTextDocument(uri);
		await vscode.window.showTextDocument(doc, {
			selection: new vscode.Range(line, 0, line, 0),
			viewColumn: vscode.ViewColumn.One
		});
	});
	context.subscriptions.push(openTaskLocationDisposable);

	// mdファイル保存時にTreeViewを自動更新
	const onSaveDisposable = vscode.workspace.onDidSaveTextDocument(async (doc) => {
		if (doc.languageId === 'markdown') {
			taskTreeProvider.refresh();
		}
	});
	context.subscriptions.push(onSaveDisposable);

	// ログエントリを挿入する共通関数
	async function insertLogEntry(dateStr: string): Promise<void> {
		const editor = vscode.window.activeTextEditor;
		if (!editor) {
			return;
		}

		const document = editor.document;
		const cursorLine = editor.selection.active.line;

		// カーソル行からタスク行を特定する（カーソル行自体がタスク行か、上方向に探す）
		let taskLineIndex = -1;
		let taskIndent = 0;
		let insertLine = cursorLine;

		for (let i = cursorLine; i >= 0; i--) {
			const lineText = document.lineAt(i).text;
			const taskMatch = lineText.match(/^(\s*)-\s*\[([ x])\]\s*/);
			if (taskMatch) {
				taskLineIndex = i;
				taskIndent = taskMatch[1].length;

				if (i === cursorLine) {
					// カーソルがタスク行にある場合、その直下に挿入
					insertLine = cursorLine;
				} else {
					// カーソルがログ行にある場合、カーソル行の下に挿入
					insertLine = cursorLine;
				}
				break;
			}

			// 日付ログ行かチェック（タスクの子要素として続いているか確認）
			const dateMatch = lineText.match(/^(\s*)-\s*\d{4}-\d{2}-\d{2}:\s*/);
			if (!dateMatch) {
				// タスク行でもログ行でもない行に到達したら探索終了
				break;
			}
		}

		if (taskLineIndex === -1) {
			return;
		}

		const logIndent = ' '.repeat(taskIndent + 4);
		const insertText = `${logIndent}- ${dateStr}: `;

		await editor.edit(editBuilder => {
			const lineEnd = document.lineAt(insertLine).range.end;
			editBuilder.insert(lineEnd, `\n${insertText}`);
		});

		// カーソルを挿入した行の末尾に移動
		const newLine = insertLine + 1;
		const newCol = insertText.length;
		const newPosition = new vscode.Position(newLine, newCol);
		editor.selection = new vscode.Selection(newPosition, newPosition);
	}

	const addTodayLogDisposable = vscode.commands.registerCommand('taski.addTodayLog', async () => {
		await insertLogEntry(getLocalDateString());
	});

	context.subscriptions.push(addTodayLogDisposable);

	const addTomorrowLogDisposable = vscode.commands.registerCommand('taski.addTomorrowLog', async () => {
		await insertLogEntry(getTomorrowDateString());
	});

	context.subscriptions.push(addTomorrowLogDisposable);

	// タスクの完了状態をトグルするコマンド
	const toggleTaskDisposable = vscode.commands.registerCommand('taski.toggleTask', async () => {
		const editor = vscode.window.activeTextEditor;
		if (!editor) {
			return;
		}

		const document = editor.document;
		const cursorLine = editor.selection.active.line;
		const lineText = document.lineAt(cursorLine).text;

		const taskMatch = lineText.match(/^(\s*-\s*\[)([ x])(\]\s*.*)/);
		if (!taskMatch) {
			return;
		}

		const newState = taskMatch[2] === 'x' ? ' ' : 'x';
		const newText = taskMatch[1] + newState + taskMatch[3];

		await editor.edit(editBuilder => {
			const lineRange = document.lineAt(cursorLine).range;
			editBuilder.replace(lineRange, newText);
		});
	});

	context.subscriptions.push(toggleTaskDisposable);

	// 今日の日付のジャーナルファイルを開くコマンド
	const openTodayJournalDisposable = vscode.commands.registerCommand('taski.openTodayJournal', async () => {
		const todayStr = getLocalDateString();
		const [year, month, day] = todayStr.split('-');

		// $HOME/taski/journal/<year>/<month>/<year>-<month>-<date>.md
		const journalDir = path.join(os.homedir(), 'taski', 'journal', year, month);
		const journalFileName = `${year}-${month}-${day}.md`;
		const journalPath = path.join(journalDir, journalFileName);

		const journalUri = vscode.Uri.file(journalPath);

		// ディレクトリが存在しない場合は作成
		try {
			await vscode.workspace.fs.stat(vscode.Uri.file(journalDir));
		} catch {
			await vscode.workspace.fs.createDirectory(vscode.Uri.file(journalDir));
		}

		// ファイルが存在しない場合は作成
		try {
			await vscode.workspace.fs.stat(journalUri);
		} catch {
			await vscode.workspace.fs.writeFile(journalUri, new TextEncoder().encode(''));
		}

		// ファイルを開く
		const doc = await vscode.workspace.openTextDocument(journalUri);
		await vscode.window.showTextDocument(doc);
	});

	context.subscriptions.push(openTodayJournalDisposable);
}

// ローカルタイムゾーンで YYYY-MM-DD を取得する関数
function getLocalDateString(): string {
	const d = new Date();
	const year = d.getFullYear();
	const month = String(d.getMonth() + 1).padStart(2, '0');
	const day = String(d.getDate()).padStart(2, '0');
	return `${year}-${month}-${day}`;
}

// ローカルタイムゾーンで明日の YYYY-MM-DD を取得する関数
function getTomorrowDateString(): string {
	const d = new Date();
	d.setDate(d.getDate() + 1);
	const year = d.getFullYear();
	const month = String(d.getMonth() + 1).padStart(2, '0');
	const day = String(d.getDate()).padStart(2, '0');
	return `${year}-${month}-${day}`;
}

