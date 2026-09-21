import { describe, expect, it } from "vitest";
import { buildApplyRequests, type ApplyTarget } from "@/lib/applyController";

function target(catalogFamily?: string): ApplyTarget {
  return {
    game_id: "game",
    game_label: "Game",
    record: {
      family: "streamline_common",
      path: "C:\\Games\\Game\\sl.common.dll",
      current_version: "2.12.0.0",
      file_description: null,
      sha256: "a".repeat(64),
    },
    target_version: "2.14.1.0",
    catalog_family: catalogFamily,
  };
}

describe("buildApplyRequests catalog family", () => {
  it("preserves the exact family supplied by an expanded Rust plan", () => {
    expect(buildApplyRequests([target("streamline_common")])[0].family).toBe("streamline_common");
  });

  it("keeps alias mapping for an unplanned detected record", () => {
    expect(buildApplyRequests([target()])[0].family).toBe("streamline");
  });
});
