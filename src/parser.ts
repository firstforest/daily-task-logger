import {
	parseTasks as wasmParseTasks,
	parseTasksAllDates as wasmParseTasksAllDates,
	buildTreeData as wasmBuildTreeData,
	buildScheduleData as wasmBuildScheduleData,
	extractTags as wasmExtractTags,
	extractFileTags as wasmExtractFileTags,
	parseWikiLinks as wasmParseWikiLinks,
	normalizeWikiName as wasmNormalizeWikiName,
	resolveWikiLink as wasmResolveWikiLink,
	wikiLinkCreatePath as wasmWikiLinkCreatePath,
	wikiLinkInitialContent as wasmWikiLinkInitialContent,
} from './pkg/parser_wasm';
import type { ParsedTask, ParsedTaskWithDate, TaskStatus } from './extension';

export interface FileInput {
	fileName: string;
	fileUri: string;
	lines: string[];
}

export interface TreeTaskData {
	status: TaskStatus;
	text: string;
	/** 判断メタデータ（`（30分・軽・@PC）`）を取り除いた本文 */
	body: string;
	/** 行末の判断メタデータ。無ければ null */
	meta: string | null;
	fileUri: string;
	line: number;
	log: string;
	date: string;
	context: string[];
}

export interface TreeFileGroup {
	fileName: string;
	fileUri: string;
	tasks: TreeTaskData[];
}

export interface TreeDateGroup {
	dateKey: string;
	label: string;
	isToday: boolean;
	fileGroups: TreeFileGroup[];
	completedCount: number;
	totalCount: number;
}

export function parseTasks(lines: string[], targetDate: string): ParsedTask[] {
	return wasmParseTasks(lines, targetDate) as ParsedTask[];
}

export function parseTasksAllDates(lines: string[]): ParsedTaskWithDate[] {
	return wasmParseTasksAllDates(lines) as ParsedTaskWithDate[];
}

export function buildTreeData(files: FileInput[], todayStr: string): TreeDateGroup[] {
	return wasmBuildTreeData(files, todayStr) as TreeDateGroup[];
}

export interface ScheduleEntry {
	taskText: string;
	taskLine: number;
	status: TaskStatus;
	logText: string;
	logLine: number;
	time: string;
	endTime: string;
	fileUri: string;
}

export function buildScheduleData(files: FileInput[], targetDate: string): ScheduleEntry[] {
	return wasmBuildScheduleData(files, targetDate) as ScheduleEntry[];
}

export function extractTags(text: string): string[] {
	return wasmExtractTags(text) as string[];
}

export function extractFileTags(lines: string[], fileName: string): string[] {
	return wasmExtractFileTags(lines, fileName) as string[];
}

export interface WikiLinkMatch {
	name: string;
	start: number;
	end: number;
}

export interface NormalizedWikiName {
	name: string;
	isJournal: boolean;
}

export function parseWikiLinks(text: string): WikiLinkMatch[] {
	return wasmParseWikiLinks(text) as WikiLinkMatch[];
}

export function normalizeWikiName(raw: string): NormalizedWikiName {
	return wasmNormalizeWikiName(raw) as NormalizedWikiName;
}

export function resolveWikiLink(name: string, candidatePaths: string[]): string | undefined {
	const got = wasmResolveWikiLink(name, candidatePaths) as string | undefined;
	return got ?? undefined;
}

export function wikiLinkCreatePath(name: string, isJournal: boolean, taskiHome: string): string {
	return wasmWikiLinkCreatePath(name, isJournal, taskiHome);
}

export function wikiLinkInitialContent(name: string): string {
	return wasmWikiLinkInitialContent(name);
}
