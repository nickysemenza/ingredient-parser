import { Dialog } from "@base-ui/react/dialog";
import { Command as Cmdk } from "cmdk";
import { Search } from "lucide-react";
import { useEffect, useState } from "react";
import { Kbd } from "../components/ui";
import { useCommandList } from "./commands";

/** ⌘K: every action in the app, searchable. */
export function Palette() {
  const [open, setOpen] = useState(false);
  const commands = useCommandList();
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setOpen((was) => !was);
      }
    };
    const onOpen = () => setOpen(true);
    window.addEventListener("keydown", onKey);
    window.addEventListener("open-palette", onOpen);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("open-palette", onOpen);
    };
  }, []);
  const groups = [...new Set(commands.map((c) => c.group))];
  return (
    <Dialog.Root open={open} onOpenChange={setOpen}>
      <Dialog.Portal>
        <Dialog.Backdrop className="fixed inset-0 z-50 bg-black/25 backdrop-blur-[2px] transition-opacity duration-150 data-ending-style:opacity-0 data-starting-style:opacity-0 dark:bg-black/50" />
        <Dialog.Popup
          aria-label="Command palette"
          className="fixed top-[14vh] left-1/2 z-50 w-[min(560px,calc(100vw-2rem))] -translate-x-1/2 overflow-hidden rounded-xl border border-line bg-evidence shadow-2xl transition-[opacity,scale] duration-150 data-ending-style:scale-[0.98] data-ending-style:opacity-0 data-starting-style:scale-[0.98] data-starting-style:opacity-0"
        >
          <Cmdk label="Command palette" loop>
            <div className="flex items-center gap-2.5 border-b border-line px-4">
              <Search className="size-4 text-faint" />
              <Cmdk.Input
                autoFocus
                placeholder="Search commands…"
                className="h-12 flex-1 bg-transparent text-[14px] text-fg outline-none placeholder:text-faint"
              />
              <Kbd>esc</Kbd>
            </div>
            <Cmdk.List className="max-h-[min(380px,60vh)] overflow-auto p-1.5">
              <Cmdk.Empty className="px-3 py-6 text-center text-[13px] text-muted">No matching commands.</Cmdk.Empty>
              {groups.map((group) => (
                <Cmdk.Group
                  key={group}
                  heading={group}
                  className="[&_[cmdk-group-heading]]:px-2.5 [&_[cmdk-group-heading]]:pt-2 [&_[cmdk-group-heading]]:pb-1 [&_[cmdk-group-heading]]:text-[11px] [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:text-faint"
                >
                  {commands
                    .filter((c) => c.group === group)
                    .map((command) => (
                      <Cmdk.Item
                        key={command.id}
                        value={`${command.group} ${command.title}`}
                        keywords={command.keywords}
                        onSelect={() => {
                          setOpen(false);
                          command.run();
                        }}
                        className="flex h-9 cursor-default items-center gap-2.5 rounded-md px-2.5 text-[13px] text-fg data-[selected=true]:bg-selection [&_svg]:size-4 [&_svg]:text-muted"
                      >
                        {command.icon}
                        <span className="flex-1">{command.title}</span>
                        {command.shortcut && <Kbd>{command.shortcut}</Kbd>}
                      </Cmdk.Item>
                    ))}
                </Cmdk.Group>
              ))}
            </Cmdk.List>
          </Cmdk>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

export const openPalette = () => window.dispatchEvent(new Event("open-palette"));
