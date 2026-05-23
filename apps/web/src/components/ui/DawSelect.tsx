import * as React from "react";
import { ChevronDown } from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
} from "./menu";
import "./DawSelect.css";

export interface DawSelectOption {
  value: string;
  label: string;
  disabled?: boolean;
  /** When set, renders a section header above this item (and a separator if not the first group). */
  groupHeader?: string;
}

interface DawSelectProps {
  value: string;
  onChange: (value: string) => void;
  options: DawSelectOption[];
  className?: string;
  disabled?: boolean;
  hideChevron?: boolean;
}

/**
 * A custom themed select component for the DAW.
 * Replaces native HTML select with a dark-themed popover.
 */
export const DawSelect: React.FC<DawSelectProps> = ({
  value,
  onChange,
  options,
  className = "",
  disabled = false,
  hideChevron = false,
}) => {
  const selectedOption = options.find((opt) => opt.value === value);

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild disabled={disabled}>
        <button
          className={`daw-select-trigger ${className}`}
          type="button"
          aria-label={selectedOption?.label || "Select option"}
        >
          <span className="daw-select-value">
            {selectedOption?.label || value}
          </span>
          {!hideChevron && <ChevronDown size={10} className="daw-select-icon" />}
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="start"
        sideOffset={4}
        className="daw-select-content min-w-[var(--radix-dropdown-menu-trigger-width)]"
      >
        <DropdownMenuRadioGroup value={value} onValueChange={onChange}>
          {options.map((option, i) => (
            <React.Fragment key={option.value}>
              {option.groupHeader && (
                <>
                  {i > 0 && <DropdownMenuSeparator />}
                  <DropdownMenuLabel>{option.groupHeader}</DropdownMenuLabel>
                </>
              )}
              <DropdownMenuRadioItem
                value={option.value}
                disabled={option.disabled}
                className="daw-select-item"
              >
                {option.label}
              </DropdownMenuRadioItem>
            </React.Fragment>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
};
