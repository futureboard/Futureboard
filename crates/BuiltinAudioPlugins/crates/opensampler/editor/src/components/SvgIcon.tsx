import type { ReactNode, SVGAttributes } from 'react'

type SvgIconProps = Omit<SVGAttributes<SVGSVGElement>, 'children'> & {
  children: ReactNode
  label?: string
  size?: number
}

export function SvgIcon({ children, label, size = 16, viewBox = '0 0 24 24', ...props }: SvgIconProps) {
  return (
    <svg
      {...props}
      width={size}
      height={size}
      viewBox={viewBox}
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      role={label ? 'img' : undefined}
      aria-hidden={label ? undefined : true}
      aria-label={label}
    >
      {children}
    </svg>
  )
}

