import { describe, expect, it } from "vitest";
import {
  beginSave,
  emptyBook,
  enqueueSaves,
  nameForFolderSave,
  pollIsCurrent,
  rememberSaved,
  rollbackDirs,
} from "./folderSave";

describe("folder save book", () => {
  it("rolls back to the last successful save and ignores a poll that started earlier", () => {
    let book = rememberSaved(emptyBook(), ["/saved/app"]);
    const pollEpoch = book.epoch;
    book = beginSave(book);
    expect(pollIsCurrent(book, pollEpoch)).toBe(false);
    expect(rollbackDirs(book)).toEqual(["/saved/app"]);
    book = rememberSaved(book, ["/saved/app", "/saved/extra"]);
    expect(rollbackDirs(book)).toEqual(["/saved/app", "/saved/extra"]);
    book = beginSave(book);
    expect(rollbackDirs(book)).toEqual(["/saved/app", "/saved/extra"]);
  });

  it("uses the typed name for a new worker only when it is valid", () => {
    expect(nameForFolderSave(null, "  draft-name  ", "dev-machine")).toBe("draft-name");
    expect(nameForFolderSave(null, "bad/name", "dev-machine")).toBe("dev-machine");
    expect(nameForFolderSave(null, "   ", "dev-machine")).toBe("dev-machine");
    expect(nameForFolderSave("saved-name", "draft-name", "dev-machine")).toBe("saved-name");
  });

  it("runs overlapping saves one at a time and rolls back to the last success", async () => {
    let book = rememberSaved(emptyBook(), ["/a"]);
    const order: string[] = [];
    let releaseFirst: () => void = () => {};
    const gate = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    let tail = Promise.resolve();
    const first = enqueueSaves(tail, async () => {
      book = beginSave(book);
      order.push("first-start");
      await gate;
      book = rememberSaved(book, ["/a", "/b"]);
      order.push("first-end");
    });
    tail = first.tail;
    const second = enqueueSaves(tail, async () => {
      order.push("second");
      const pollEpoch = book.epoch;
      book = beginSave(book);
      expect(pollIsCurrent(book, pollEpoch)).toBe(false);
      expect(rollbackDirs(book)).toEqual(["/a", "/b"]);
    });
    expect(order).toEqual([]);
    releaseFirst();
    await second.result;
    expect(order).toEqual(["first-start", "first-end", "second"]);
  });
});
