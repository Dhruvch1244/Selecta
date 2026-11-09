import { describe, expect, it } from "vitest";
// Written by the Rust core's tests (crates/selecta-core/src/fx.rs): the single source of effect ids.
import catalog from "../../protocol/src/generated/fx-catalog.json";
import { FAMILIES } from "./fx";

describe("fx catalog coverage", () => {
  it("implements every effect family the Rust catalog (and so the director) can emit", () => {
    const missing = [...new Set(catalog.map((c) => c.family))].filter((f) => !FAMILIES.includes(f));
    expect(missing).toEqual([]);
  });
});
