import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { JsonView, RecipeScale } from "./index";

describe("shared recipe presentation", () => {
  it("sends desktop selection to the caller and respects pending work", () => {
    const onChange = vi.fn();
    const view = render(<RecipeScale value={1} onChange={onChange} />);
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "2" } });
    expect(onChange).toHaveBeenCalledWith(2);
    view.rerender(<RecipeScale value={2} onChange={onChange} disabled />);
    expect(screen.getByRole("combobox")).toBeDisabled();
  });

  it("supports demo presets and rejects invalid custom factors", () => {
    const onChange = vi.fn();
    render(<RecipeScale custom value={1} onChange={onChange} />);
    expect(screen.getByRole("button", { name: "1x" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    fireEvent.click(screen.getByRole("button", { name: "½x" }));
    expect(onChange).toHaveBeenLastCalledWith(0.5);
    const input = screen.getByRole("spinbutton", { name: "Recipe scale" });
    for (const value of ["", "0", "-1"]) {
      fireEvent.change(input, { target: { value } });
    }
    expect(onChange).toHaveBeenCalledTimes(1);
    fireEvent.change(input, { target: { value: "1.7" } });
    expect(onChange).toHaveBeenLastCalledWith(1.7);
  });

  it("shows imported JSON as text without interpreting markup", () => {
    const value = { name: '<script>alert("recipe")</script>', optional: false };
    const { container } = render(<JsonView value={value} />);
    expect(container.querySelector("pre")?.textContent).toBe(
      JSON.stringify(value, null, 2),
    );
    expect(container.querySelector("script")).toBeNull();
  });
});
