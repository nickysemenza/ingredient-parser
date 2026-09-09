import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { Split, useStored, VirtualList } from "./components";
afterEach(() => {
  cleanup();
  localStorage.clear();
});
function Preference() {
  const [value] = useStored("mode", "Parser", ["Parser", "Cookbooks"]);
  return <span>{value}</span>;
}
describe("persisted preferences", () => {
  for (const value of ["{broken", "{}", "42", '"Other"'])
    it(`rejects invalid preference ${value}`, () => {
      localStorage.setItem("mode", value);
      render(<Preference />);
      expect(screen.getByText("Parser")).toBeInTheDocument();
    });
  it("rejects nonnumeric split values and supports keyboard resizing", () => {
    localStorage.setItem("v1:split:example", "{}");
    render(<Split id="example" left="source" right="result" />);
    const separator = screen.getByRole("separator");
    expect(separator).toHaveAttribute("aria-valuenow", "45");
    fireEvent.keyDown(separator, { key: "ArrowRight" });
    expect(separator).toHaveAttribute("aria-valuenow", "47");
  });
});
it("virtualizes long lists and supports keyboard selection", () => {
  const rows = Array.from({ length: 10000 }, (_, i) => i);
  let selected = -1;
  render(
    <VirtualList
      label="Many rows"
      rows={rows}
      selected={0}
      onSelect={(v) => {
        selected = v;
      }}
      render={(v) => <span>{v}</span>}
    />,
  );
  expect(screen.getAllByRole("option").length).toBeLessThan(100);
  fireEvent.keyDown(screen.getByRole("listbox"), { key: "End" });
  expect(selected).toBe(9999);
});
