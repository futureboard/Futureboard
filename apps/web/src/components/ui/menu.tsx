import * as DropdownPrimitive from "@radix-ui/react-dropdown-menu";
import { Check } from "lucide-react";
import {
  forwardRef,
  type ComponentPropsWithoutRef,
  type ElementRef,
  type ReactNode,
} from "react";

export const DropdownMenu = DropdownPrimitive.Root;
export const DropdownMenuTrigger = DropdownPrimitive.Trigger;
export const DropdownMenuGroup = DropdownPrimitive.Group;
export const DropdownMenuSub = DropdownPrimitive.Sub;
export const DropdownMenuRadioGroup = DropdownPrimitive.RadioGroup;
export const DropdownMenuPortal = DropdownPrimitive.Portal;

// ── Content ───────────────────────────────────────────────────────────────────

type ContentProps = ComponentPropsWithoutRef<typeof DropdownPrimitive.Content>;

const baseSurface = [
  "z-[9999] min-w-[220px] max-w-[320px] rounded-[10px]",
  "border border-white/[0.08] bg-[#171c23] p-1.5",
  "text-[12px] text-daw-text",
  "shadow-[0_18px_48px_rgba(0,0,0,0.48)] outline-none",
].join(" ");

export const DropdownMenuContent = forwardRef<
  ElementRef<typeof DropdownPrimitive.Content>,
  ContentProps
>(({ className = "", align = "start", sideOffset = 6, collisionPadding = 12, ...props }, ref) => (
  <DropdownPrimitive.Portal>
    <DropdownPrimitive.Content
      ref={ref}
      align={align}
      sideOffset={sideOffset}
      collisionPadding={collisionPadding}
      className={`${baseSurface} ${className}`}
      {...props}
    />
  </DropdownPrimitive.Portal>
));
DropdownMenuContent.displayName = "DropdownMenuContent";

// ── SubContent ────────────────────────────────────────────────────────────────

export const DropdownMenuSubTrigger = forwardRef<
  ElementRef<typeof DropdownPrimitive.SubTrigger>,
  ComponentPropsWithoutRef<typeof DropdownPrimitive.SubTrigger> & {
    icon?: React.ElementType;
    inset?: boolean;
  }
>(({ className = "", children, icon: Icon, inset, ...props }, ref) => (
  <DropdownPrimitive.SubTrigger
    ref={ref}
    className={[
      "group relative flex h-7 cursor-default select-none items-center gap-2 rounded-[7px]",
      "px-2 text-[12px] font-medium text-daw-dim outline-none",
      "data-[highlighted]:bg-white/[0.06] data-[highlighted]:text-daw-text",
      "data-[state=open]:bg-white/[0.06] data-[state=open]:text-daw-text",
      inset ? "pl-7" : "",
      className,
    ].join(" ")}
    {...props}
  >
    {Icon ? <Icon size={13} className="shrink-0 opacity-80" /> : null}
    <span className="min-w-0 flex-1 truncate">{children}</span>
    <span className="ml-2 text-[10px] text-daw-faint">›</span>
  </DropdownPrimitive.SubTrigger>
));
DropdownMenuSubTrigger.displayName = "DropdownMenuSubTrigger";

export const DropdownMenuSubContent = forwardRef<
  ElementRef<typeof DropdownPrimitive.SubContent>,
  ComponentPropsWithoutRef<typeof DropdownPrimitive.SubContent>
>(({ className = "", ...props }, ref) => (
  <DropdownPrimitive.SubContent
    ref={ref}
    className={`${baseSurface} ${className}`}
    {...props}
  />
));
DropdownMenuSubContent.displayName = "DropdownMenuSubContent";

// ── Item ──────────────────────────────────────────────────────────────────────

type ItemProps = ComponentPropsWithoutRef<typeof DropdownPrimitive.Item> & {
  icon?: React.ElementType;
  shortcut?: string;
  danger?: boolean;
  inset?: boolean;
};

export const DropdownMenuItem = forwardRef<
  ElementRef<typeof DropdownPrimitive.Item>,
  ItemProps
>(({ className = "", icon: Icon, shortcut, danger, inset, children, ...props }, ref) => (
  <DropdownPrimitive.Item
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
    {shortcut ? <DropdownMenuShortcut>{shortcut}</DropdownMenuShortcut> : null}
  </DropdownPrimitive.Item>
));
DropdownMenuItem.displayName = "DropdownMenuItem";

// ── CheckboxItem ──────────────────────────────────────────────────────────────

export const DropdownMenuCheckboxItem = forwardRef<
  ElementRef<typeof DropdownPrimitive.CheckboxItem>,
  ComponentPropsWithoutRef<typeof DropdownPrimitive.CheckboxItem> & {
    icon?: React.ElementType;
    shortcut?: string;
  }
>(({ className = "", children, checked, icon: Icon, shortcut, ...props }, ref) => (
  <DropdownPrimitive.CheckboxItem
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
      <DropdownPrimitive.ItemIndicator>
        <Check size={12} />
      </DropdownPrimitive.ItemIndicator>
    </span>
    {Icon ? <Icon size={13} className="shrink-0 opacity-80" /> : null}
    <span className="min-w-0 flex-1 truncate">{children}</span>
    {shortcut ? <DropdownMenuShortcut>{shortcut}</DropdownMenuShortcut> : null}
  </DropdownPrimitive.CheckboxItem>
));
DropdownMenuCheckboxItem.displayName = "DropdownMenuCheckboxItem";

// ── RadioItem ─────────────────────────────────────────────────────────────────

export const DropdownMenuRadioItem = forwardRef<
  ElementRef<typeof DropdownPrimitive.RadioItem>,
  ComponentPropsWithoutRef<typeof DropdownPrimitive.RadioItem> & {
    shortcut?: string;
  }
>(({ className = "", children, shortcut, ...props }, ref) => (
  <DropdownPrimitive.RadioItem
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
      <DropdownPrimitive.ItemIndicator>
        <span className="h-1.5 w-1.5 rounded-full bg-current" />
      </DropdownPrimitive.ItemIndicator>
    </span>
    <span className="min-w-0 flex-1 truncate">{children}</span>
    {shortcut ? <DropdownMenuShortcut>{shortcut}</DropdownMenuShortcut> : null}
  </DropdownPrimitive.RadioItem>
));
DropdownMenuRadioItem.displayName = "DropdownMenuRadioItem";

// ── Label / Separator / Shortcut ──────────────────────────────────────────────

export const DropdownMenuLabel = forwardRef<
  ElementRef<typeof DropdownPrimitive.Label>,
  ComponentPropsWithoutRef<typeof DropdownPrimitive.Label>
>(({ className = "", ...props }, ref) => (
  <DropdownPrimitive.Label
    ref={ref}
    className={[
      "px-2 py-1.5 text-[10px] font-bold uppercase tracking-[0.12em] text-daw-faint",
      className,
    ].join(" ")}
    {...props}
  />
));
DropdownMenuLabel.displayName = "DropdownMenuLabel";

export const DropdownMenuSeparator = forwardRef<
  ElementRef<typeof DropdownPrimitive.Separator>,
  ComponentPropsWithoutRef<typeof DropdownPrimitive.Separator>
>(({ className = "", ...props }, ref) => (
  <DropdownPrimitive.Separator
    ref={ref}
    className={`my-1 h-px bg-white/[0.07] ${className}`}
    {...props}
  />
));
DropdownMenuSeparator.displayName = "DropdownMenuSeparator";

export function DropdownMenuShortcut({ children }: { children: ReactNode }) {
  return (
    <span className="ml-auto pl-3 text-[11px] tracking-wide text-daw-faint">
      {children}
    </span>
  );
}
