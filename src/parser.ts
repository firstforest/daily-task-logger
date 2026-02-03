import { parseTasks as wasmParseTasks, parseTasksAllDates as wasmParseTasksAllDates } from './pkg/parser_wasm';
import type { ParsedTask, ParsedTaskWithDate } from './extension';

export function parseTasks(lines: string[], targetDate: string): ParsedTask[] {
	return wasmParseTasks(lines, targetDate) as ParsedTask[];
}

export function parseTasksAllDates(lines: string[]): ParsedTaskWithDate[] {
	return wasmParseTasksAllDates(lines) as ParsedTaskWithDate[];
}
