import { APP_MENUS, type AppMenuGroup, type AppMenuItem } from "./menuItems";

export type CommandItem = {
  id: string;
  label: string;
  action: string;
  group: string;
  accelerator?: string;
  icon?: string;
  enabled?: boolean;
  danger?: boolean;
  keywords?: string[];
  description?: string;
};

export function findMenuItemByAction(
  menus: readonly AppMenuGroup[],
  action: string
): AppMenuItem | undefined {
  for (const menu of menus) {
    const found = findMenuItemInTree(menu.children, action);
    if (found) return found;
  }

  return undefined;
}

function findMenuItemInTree(
  items: readonly AppMenuItem[],
  action: string
): AppMenuItem | undefined {
  for (const item of items) {
    if (item.type === "separator") continue;

    if (item.type === "submenu") {
      const found = findMenuItemInTree(item.children, action);
      if (found) return found;
      continue;
    }

    if (item.action === action) return item;
  }

  return undefined;
}

export function getTopLevelMenuLabels() {
  return APP_MENUS.map((menu) => ({
    id: menu.id,
    label: menu.label,
  }));
}

export function flattenMenuItems(): CommandItem[] {
  const result: CommandItem[] = [];

  for (const group of APP_MENUS) {
    const traverse = (items: readonly AppMenuItem[], groupLabel: string) => {
      for (const item of items) {
        if (item.type === "separator") continue;

        if (item.type === "submenu") {
          traverse(item.children, `${groupLabel} > ${item.label}`);
        } else if (item.action) {
          result.push({
            id: item.id,
            label: item.label,
            action: item.action,
            group: groupLabel,
            accelerator: item.accelerator,
            icon: item.icon,
            enabled: item.enabled ?? true,
            danger: item.danger,
            description: item.description,
            // Could generate keywords from label or description
          });
        }
      }
    };

    traverse(group.children, group.label);
  }

  return result;
}
