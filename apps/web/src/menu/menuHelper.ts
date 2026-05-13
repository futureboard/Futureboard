import { APP_MENUS, type AppMenuGroup, type AppMenuItem } from "./menuItems";

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
