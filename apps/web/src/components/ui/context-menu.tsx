import * as ContextMenuPrimitive from "@radix-ui/react-context-menu";
import { Check } from "lucide-react";
import {
  forwardRef,
  type ComponentPropsWithoutRef,
  type ElementRef,
  type ReactNode,
} from "react";

export const ContextMenu = ContextMenuPrimitive.Root;
export const ContextMenuTrigger = ContextMenuPrimitive.Trigger;
export const ContextMenuGroup = ContextMenuPrimitive.Group;
export const ContextMenuSub = ContextMenuPrimitive.Sub;
export const ContextMenuRadioGroup = ContextMenuPrimitive.RadioGroup;
export const ContextMenuPortal = ContextMenuPrimitive.Portal;

const baseSurface = [
  "z-[1000] min-w-[220px] max-w-[320px] rounded-[10px]",
  "border border-white/[0.08] bg-[#171c23] p-1.5",
  "text-[12px] text-daw-text",
  "shadow-[0_18px_48px_rgba(0,0,0,0.48)] outline-none",
].join(" ");

export const ContextMenuContent = forwardRef<
  ElementRef<typeof ContextMenuPrimitive.Content>,
  ComponentPropsWithoutRef<typeof ContextMenuPrimitive.Content>
>(({ className = "", ...props }, ref) => (
  <ContextMenuPrimitive.Portal>
    <ContextMenuPrimitive.Content
      ref={ref}
      className={`${baseSurface} ${className}`}
      {...props}
    />
  </ContextMenuPrimitive.Portal>
));
ContextMenuContent.displayName = "ContextMenuContent";

export const ContextMenuSubTrigger = forwardRef<
  ElementRef<typeof ContextMenuPrimitive.SubTrigger>,
  ComponentPropsWithoutRef<typeof ContextMenuPrimitive.SubTrigger> & {
    icon?: React.ElementType;
  }
>(({ className = "", icon: Icon, children, ...props }, ref) => (
  <ContextMenuPrimitive.SubTrigger
    ref={ref}
    className={[
      "group relative flex h-7 cursor-default select-none items-center gap-2 rounded-[7px]",
      "px-2 text-[12px] font-medium text-daw-dim outline-none",
      "data-[highlighted]:bg-white/[0.06] data-[highlighted]:text-daw-text",
      "data-[state=open]:bg-white/[0.06] data-[state=open]:text-daw-text",
      className,
    ].join(" ")}
    {...props}
  >
    {Icon ? <Icon size={13} className="shrink-0 opacity-80" /> : null}
    <span className="min-w-0 flex-1 truncate">{children}</span>
    <span className="ml-2 text-[10px] text-daw-faint">›</span>
  </ContextMenuPrimitive.SubTrigger>
));
ContextMenuSubTrigger.displayName = "ContextMenuSubTrigger";

export const ContextMenuSubContent = forwardRef<
  ElementRef<typeof ContextMenuPrimitive.SubContent>,
  ComponentPropsWithoutRef<typeof ContextMenuPrimitive.SubContent>
>(({ className = "", ...props }, ref) => (
  <ContextMenuPrimitive.SubContent
    ref={ref}
    className={`${baseSurface} ${className}`}
    {...props}
  />
));
ContextMenuSubContent.displayName = "ContextMenuSubContent";

type ItemProps = ComponentPropsWithoutRef<typeof ContextMenuPrimitive.Item> & {
  icon?: React.ElementType;
  shortcut?: string;
  danger?: boolean;
  inset?: boolean;
};

export const ContextMenuItem = forwardRef<
  ElementRef<typeof ContextMenuPrimitive.Item>,
  ItemProps
>(({ className = "", icon: Icon, shortcut, danger, inset, children, ...props }, ref) => (
  <ContextMenuPrimitive.Item
    ref={ref}
    className={[
      "relative flex h-7 cursor-default select-none items-center gap-2 rounded-[7px] px-2",
      "text-[12px] font-medium outline-none",
      danger ? "text-daw-red" : "text-daw-dim",
      danger
        ? "data-[highlighted]:bg-daw-red/15 data-[highlighted]:text-daw-red"
        : "data-[highlighted]:bg-white/[0.06] data-[highlighted]:text-daw-text",
      "data-[disabled]:pointer-events-none data-[disabled]:opacity-40",
      inset ? "pl-7" : "",
      className,
    ].join(" ")}
    {...props}
  >
    {Icon ? <Icon size={13} className="shrink-0 opacity-80" /> : null}
    <span className="min-w-0 flex-1 truncate">{children}</span>
    {shortcut ? <ContextMenuShortcut>{shortcut}</ContextMenuShortcut> : null}
  </ContextMenuPrimitive.Item>
));
ContextMenuItem.displayName = "ContextMenuItem";

export const ContextMenuCheckboxItem = forwardRef<
  ElementRef<typeof ContextMenuPrimitive.CheckboxItem>,
  ComponentPropsWithoutRef<typeof ContextMenuPrimitive.CheckboxItem> & {
    shortcut?: string;
  }
>(({ className = "", checked, children, shortcut, ...props }, ref) => (
  <ContextMenuPrimitive.CheckboxItem
    ref={ref}
    checked={checked}
    className={[
      "relative flex h-7 cursor-default select-none items-center gap-2 rounded-[7px] pl-7 pr-2",
      "text-[12px] font-medium text-daw-dim outline-none",
      "data-[highlighted]:bg-white/[0.06] data-[highlighted]:text-daw-text",
      "data-[state=checked]:text-daw-text",
      "data-[disabled]:pointer-events-none data-[disabled]:opacity-40",
      className,
    ].join(" ")}
    {...props}
  >
    <span className="absolute left-2 flex h-3.5 w-3.5 items-center justify-center text-daw-accent-h">
      <ContextMenuPrimitive.ItemIndicator>
        <Check size={12} />
      </ContextMenuPrimitive.ItemIndicator>
    </span>
    <span className="min-w-0 flex-1 truncate">{children}</span>
    {shortcut ? <ContextMenuShortcut>{shortcut}</ContextMenuShortcut> : null}
  </ContextMenuPrimitive.CheckboxItem>
));
ContextMenuCheckboxItem.displayName = "ContextMenuCheckboxItem";

export const ContextMenuRadioItem = forwardRef<
  ElementRef<typeof ContextMenuPrimitive.RadioItem>,
  ComponentPropsWithoutRef<typeof ContextMenuPrimitive.RadioItem>
>(({ className = "", children, ...props }, ref) => (
  <ContextMenuPrimitive.RadioItem
    ref={ref}
    className={[
      "relative flex h-7 cursor-default select-none items-center gap-2 rounded-[7px] pl-7 pr-2",
      "text-[12px] font-medium text-daw-dim outline-none",
      "data-[highlighted]:bg-white/[0.06] data-[highlighted]:text-daw-text",
      "data-[state=checked]:text-daw-text",
      "data-[disabled]:pointer-events-none data-[disabled]:opacity-40",
      className,
    ].join(" ")}
    {...props}
  >
    <span className="absolute left-2 flex h-3.5 w-3.5 items-center justify-center text-daw-accent-h">
      <ContextMenuPrimitive.ItemIndicator>
        <span className="h-1.5 w-1.5 rounded-full bg-current" />
      </ContextMenuPrimitive.ItemIndicator>
    </span>
    <span className="min-w-0 flex-1 truncate">{children}</span>
  </ContextMenuPrimitive.RadioItem>
));
ContextMenuRadioItem.displayName = "ContextMenuRadioItem";

export const ContextMenuLabel = forwardRef<
  ElementRef<typeof ContextMenuPrimitive.Label>,
  ComponentPropsWithoutRef<typeof ContextMenuPrimitive.Label>
>(({ className = "", ...props }, ref) => (
  <ContextMenuPrimitive.Label
    ref={ref}
    className={[
      "px-2 py-1.5 text-[10px] font-bold uppercase tracking-[0.12em] text-daw-faint",
      className,
    ].join(" ")}
    {...props}
  />
));
ContextMenuLabel.displayName = "ContextMenuLabel";

export const ContextMenuSeparator = forwardRef<
  ElementRef<typeof ContextMenuPrimitive.Separator>,
  ComponentPropsWithoutRef<typeof ContextMenuPrimitive.Separator>
>(({ className = "", ...props }, ref) => (
  <ContextMenuPrimitive.Separator
    ref={ref}
    className={`my-1 h-px bg-white/[0.07] ${className}`}
    {...props}
  />
));
ContextMenuSeparator.displayName = "ContextMenuSeparator";

export function ContextMenuShortcut({ children }: { children: ReactNode }) {
  return (
    <span className="ml-auto pl-3 text-[11px] tracking-wide text-daw-faint">
      {children}
    </span>
  );
}
