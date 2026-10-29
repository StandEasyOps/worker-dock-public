import type { WorkerDir } from "./worker";

export const MAX_WORKER_NAME = 64;

export type FolderBook = {
  epoch: number;
  lastSaved: string[];
};

export function emptyBook(): FolderBook {
  return { epoch: 0, lastSaved: [] };
}

export function isValidWorkerName(name: string): boolean {
  if (!name || [...name].length > MAX_WORKER_NAME) {
    return false;
  }
  for (const char of name) {
    const code = char.codePointAt(0) ?? 0;
    if (char === "/" || char === "\\" || code <= 0x1f || (code >= 0x7f && code <= 0x9f)) {
      return false;
    }
  }
  return true;
}

export function nameForFolderSave(
  savedName: string | null,
  typedName: string,
  suggestedName: string,
): string {
  if (savedName !== null) {
    return savedName;
  }
  const typed = typedName.trim();
  if (isValidWorkerName(typed)) {
    return typed;
  }
  return suggestedName;
}

export function beginSave(book: FolderBook): FolderBook {
  return { ...book, epoch: book.epoch + 1 };
}

export function pollIsCurrent(book: FolderBook, startedEpoch: number): boolean {
  return startedEpoch === book.epoch;
}

export function rememberSaved(book: FolderBook, dirs: string[]): FolderBook {
  return { ...book, lastSaved: dirs.slice() };
}

export function rollbackDirs(book: FolderBook): string[] {
  return book.lastSaved.slice();
}

export function enqueueSaves<T>(
  tail: Promise<void>,
  task: () => Promise<T>,
): { tail: Promise<void>; result: Promise<T> } {
  const result = tail.then(task, task);
  return {
    result,
    tail: result.then(
      () => undefined,
      () => undefined,
    ),
  };
}

export function copyDirs(dirs: WorkerDir[]): WorkerDir[] {
  return dirs.map((folder) => ({ ...folder }));
}
