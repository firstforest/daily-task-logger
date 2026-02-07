import * as vscode from 'vscode';
import * as path from 'path';
import * as os from 'os';
import { parseTasksAllDates, ParsedTaskWithDate } from './extension';

// TreeViewのノード種別
type TreeNodeType = 'date' | 'file' | 'task' | 'log';

interface TaskData {
	isCompleted: boolean;
	text: string;
	fileUri: string;
	line: number;
	log: string;
	date: string;
}

interface FileTaskGroup {
	fileName: string;
	fileUri: string;
	tasks: TaskData[];
}

// TreeItemとして表示するノード
export class TaskTreeItem extends vscode.TreeItem {
	constructor(
		public readonly nodeType: TreeNodeType,
		public readonly label: string,
		public readonly collapsibleState: vscode.TreeItemCollapsibleState,
		public readonly dateKey?: string,
		public readonly fileGroup?: FileTaskGroup,
		public readonly task?: TaskData,
		public readonly isToday?: boolean
	) {
		super(label, collapsibleState);
		this.contextValue = nodeType;

		if (nodeType === 'date') {
			// 今日: 緑、過去: オレンジ、日付なし: グレー
			if (isToday) {
				this.iconPath = new vscode.ThemeIcon('calendar', new vscode.ThemeColor('charts.green'));
			} else if (dateKey === '') {
				this.iconPath = new vscode.ThemeIcon('calendar', new vscode.ThemeColor('disabledForeground'));
			} else {
				this.iconPath = new vscode.ThemeIcon('calendar', new vscode.ThemeColor('charts.orange'));
			}
		} else if (nodeType === 'file') {
			this.iconPath = new vscode.ThemeIcon('file-text', new vscode.ThemeColor('charts.blue'));
		} else if (nodeType === 'task' && task) {
			// 完了: 緑、未完了: 黄色
			if (task.isCompleted) {
				this.iconPath = new vscode.ThemeIcon('pass', new vscode.ThemeColor('charts.green'));
			} else {
				this.iconPath = new vscode.ThemeIcon('circle-outline', new vscode.ThemeColor('charts.yellow'));
			}
			// タスクをクリックしたらファイルを開く
			this.command = {
				command: 'taski.openTaskLocation',
				title: 'タスクの場所を開く',
				arguments: [task.fileUri, task.line]
			};
		} else if (nodeType === 'log') {
			this.iconPath = new vscode.ThemeIcon('note', new vscode.ThemeColor('charts.purple'));
		}
	}
}

export class TaskTreeProvider implements vscode.TreeDataProvider<TaskTreeItem> {
	private _onDidChangeTreeData = new vscode.EventEmitter<TaskTreeItem | undefined | null | void>();
	readonly onDidChangeTreeData = this._onDidChangeTreeData.event;

	private dateMap: Map<string, FileTaskGroup[]> = new Map();
	private todayStr: string = '';

	constructor() {
		this.todayStr = this.getLocalDateString();
	}

	refresh(): void {
		this.todayStr = this.getLocalDateString();
		this._onDidChangeTreeData.fire();
	}

	private getLocalDateString(): string {
		const d = new Date();
		const year = d.getFullYear();
		const month = String(d.getMonth() + 1).padStart(2, '0');
		const day = String(d.getDate()).padStart(2, '0');
		return `${year}-${month}-${day}`;
	}

	async getChildren(element?: TaskTreeItem): Promise<TaskTreeItem[]> {
		if (!element) {
			// ルートレベル: 日付ノードを返す
			await this.collectAllTasks();
			return this.getDateNodes();
		}

		if (element.nodeType === 'date' && element.dateKey !== undefined) {
			// 日付ノードの子: ファイルノードを返す
			return this.getFileNodes(element.dateKey);
		}

		if (element.nodeType === 'file' && element.fileGroup) {
			// ファイルノードの子: タスクノードを返す
			return this.getTaskNodes(element.fileGroup);
		}

		if (element.nodeType === 'task' && element.task?.log) {
			// タスクノードの子: ログノードを返す
			return [new TaskTreeItem(
				'log',
				element.task.log,
				vscode.TreeItemCollapsibleState.None
			)];
		}

		return [];
	}

	getTreeItem(element: TaskTreeItem): vscode.TreeItem {
		return element;
	}

	private getDateNodes(): TaskTreeItem[] {
		const nodes: TaskTreeItem[] = [];

		// 今日のタスク
		const todayGroups = this.dateMap.get(this.todayStr);
		if (todayGroups && todayGroups.length > 0) {
			// 進捗を計算
			let totalTasks = 0;
			let completedTasks = 0;
			for (const group of todayGroups) {
				for (const task of group.tasks) {
					totalTasks++;
					if (task.isCompleted) {
						completedTasks++;
					}
				}
			}
			nodes.push(new TaskTreeItem(
				'date',
				`今日 (${this.todayStr}) (${completedTasks}/${totalTasks})`,
				vscode.TreeItemCollapsibleState.Expanded,
				this.todayStr,
				undefined,
				undefined,
				true
			));
		}

		// その他の日付（新しい順）
		const otherDates = [...this.dateMap.keys()]
			.filter(d => d !== this.todayStr && d !== '')
			.sort()
			.reverse();

		for (const date of otherDates) {
			const groups = this.dateMap.get(date)!;
			// 未完了タスクがあるかチェック
			const hasIncompleteTasks = groups.some(g => g.tasks.some(t => !t.isCompleted));
			if (hasIncompleteTasks) {
				nodes.push(new TaskTreeItem(
					'date',
					date,
					vscode.TreeItemCollapsibleState.Collapsed,
					date,
					undefined,
					undefined,
					false
				));
			}
		}

		// 日付なし
		const noDateGroups = this.dateMap.get('');
		if (noDateGroups && noDateGroups.length > 0) {
			const hasIncompleteTasks = noDateGroups.some(g => g.tasks.some(t => !t.isCompleted));
			if (hasIncompleteTasks) {
				nodes.push(new TaskTreeItem(
					'date',
					'日付なし',
					vscode.TreeItemCollapsibleState.Collapsed,
					'',
					undefined,
					undefined,
					false
				));
			}
		}

		return nodes;
	}

	private getFileNodes(dateKey: string): TaskTreeItem[] {
		const groups = this.dateMap.get(dateKey);
		if (!groups) {
			return [];
		}

		const isToday = dateKey === this.todayStr;
		const nodes: TaskTreeItem[] = [];

		for (const group of groups) {
			// 今日以外は未完了タスクのみ表示
			const visibleTasks = isToday ? group.tasks : group.tasks.filter(t => !t.isCompleted);
			if (visibleTasks.length === 0) {
				continue;
			}

			const filteredGroup: FileTaskGroup = {
				fileName: group.fileName,
				fileUri: group.fileUri,
				tasks: visibleTasks
			};

			nodes.push(new TaskTreeItem(
				'file',
				group.fileName,
				vscode.TreeItemCollapsibleState.Expanded,
				dateKey,
				filteredGroup
			));
		}

		return nodes;
	}

	private getTaskNodes(fileGroup: FileTaskGroup): TaskTreeItem[] {
		// 完了タスクを後ろにソート
		const sorted = [...fileGroup.tasks].sort((a, b) => Number(a.isCompleted) - Number(b.isCompleted));

		return sorted.map(task => new TaskTreeItem(
			'task',
			task.text,
			task.log ? vscode.TreeItemCollapsibleState.Expanded : vscode.TreeItemCollapsibleState.None,
			undefined,
			undefined,
			task
		));
	}

	private async collectAllTasks(): Promise<void> {
		const allFileUris = await this.findAllMarkdownUris();
		this.dateMap = new Map();

		for (const fileUri of allFileUris) {
			const doc = await vscode.workspace.openTextDocument(fileUri);
			const lines: string[] = [];
			for (let i = 0; i < doc.lineCount; i++) {
				lines.push(doc.lineAt(i).text);
			}
			const tasksInFile = parseTasksAllDates(lines);

			if (tasksInFile.length > 0) {
				const relativePath = vscode.workspace.asRelativePath(fileUri);
				const fileName = path.basename(relativePath);

				// 日付ごとにグループ化
				const byDate = new Map<string, ParsedTaskWithDate[]>();
				for (const t of tasksInFile) {
					let arr = byDate.get(t.date);
					if (!arr) {
						arr = [];
						byDate.set(t.date, arr);
					}
					arr.push(t);
				}

				for (const [date, tasks] of byDate) {
					let groups = this.dateMap.get(date);
					if (!groups) {
						groups = [];
						this.dateMap.set(date, groups);
					}
					groups.push({
						fileName,
						fileUri: fileUri.toString(),
						tasks: tasks.map(t => ({
							isCompleted: t.isCompleted,
							text: t.text,
							fileUri: fileUri.toString(),
							line: t.line,
							log: t.log,
							date: t.date
						}))
					});
				}
			}
		}
	}

	private matchesExcludePattern(filePath: string, patterns: string[]): boolean {
		for (const pattern of patterns) {
			const regexStr = pattern
				.replace(/[.+^${}()|[\]\\]/g, '\\$&')
				.replace(/\*\*/g, '<<GLOBSTAR>>')
				.replace(/\*/g, '[^/]*')
				.replace(/<<GLOBSTAR>>/g, '.*');
			const regex = new RegExp(regexStr);
			if (regex.test(filePath)) {
				return true;
			}
		}
		return false;
	}

	private async findMarkdownFilesInDirectory(dirUri: vscode.Uri, excludePatterns: string[] = []): Promise<vscode.Uri[]> {
		const results: vscode.Uri[] = [];
		const entries = await vscode.workspace.fs.readDirectory(dirUri);
		for (const [name, type] of entries) {
			const childUri = vscode.Uri.joinPath(dirUri, name);
			if (type === vscode.FileType.Directory) {
				if (name === 'node_modules') {
					continue;
				}
				if (excludePatterns.length > 0 && this.matchesExcludePattern(childUri.fsPath, excludePatterns)) {
					continue;
				}
				const nested = await this.findMarkdownFilesInDirectory(childUri, excludePatterns);
				results.push(...nested);
			} else if (type === vscode.FileType.File && name.endsWith('.md')) {
				if (excludePatterns.length > 0 && this.matchesExcludePattern(childUri.fsPath, excludePatterns)) {
					continue;
				}
				results.push(childUri);
			}
		}
		return results;
	}

	private async findAllMarkdownUris(): Promise<vscode.Uri[]> {
		const config = vscode.workspace.getConfiguration('taski');
		const excludeDirs: string[] = config.get<string[]>('excludeDirectories', []);
		const includeWorkspace: boolean = config.get<boolean>('includeWorkspace', false);

		const seen = new Set<string>();
		const allFileUris: vscode.Uri[] = [];

		// ワークスペースのスキャン（設定で有効な場合のみ）
		if (includeWorkspace) {
			const excludePatterns = ['**/node_modules/**', ...excludeDirs];
			const excludeGlob = `{${excludePatterns.join(',')}}`;
			const workspaceFiles = await vscode.workspace.findFiles('**/*.md', excludeGlob);

			for (const uri of workspaceFiles) {
				const key = uri.toString();
				if (!seen.has(key)) {
					seen.add(key);
					allFileUris.push(uri);
				}
			}
		}

		// 開いているマークダウンドキュメントは常に含める
		for (const doc of vscode.workspace.textDocuments) {
			if (doc.uri.scheme === 'file' && doc.languageId === 'markdown') {
				const key = doc.uri.toString();
				if (!seen.has(key)) {
					seen.add(key);
					allFileUris.push(doc.uri);
				}
			}
		}

		const userAdditionalDirs: string[] = config.get<string[]>('additionalDirectories', []);
		const defaultTaskiDir = path.join(os.homedir(), 'taski');
		const additionalDirs = [defaultTaskiDir, ...userAdditionalDirs];
		for (const dirPath of additionalDirs) {
			const dirUri = vscode.Uri.file(dirPath);
			try {
				const mdFiles = await this.findMarkdownFilesInDirectory(dirUri, excludeDirs);
				for (const uri of mdFiles) {
					const key = uri.toString();
					if (!seen.has(key)) {
						seen.add(key);
						allFileUris.push(uri);
					}
				}
			} catch {
				// ディレクトリが存在しない場合などはスキップ
			}
		}

		return allFileUris;
	}
}
