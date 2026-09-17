<script lang="ts" module>
  const paths = {
    backup: "M12 16V4 M7 9l5-5 5 5 M4 15v3a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-3",
    restore: "M12 4v12 M7 11l5 5 5-5 M4 15v3a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-3",
    browse: "M4 6h16 M4 12h16 M4 18h10",
    archive: "M3 5h18v4H3z M5 9v10h14V9 M10 13h4",
    folder: "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z",
    file: "M6 3h8l4 4v14H6z M14 3v4h4",
    plus: "M12 5v14 M5 12h14",
    check: "M5 12.5l4.5 4.5L19 7.5",
    alert: "M12 8v5 M12 16.5v.5 M10.3 3.9 2.6 17.5A2 2 0 0 0 4.3 20.5h15.4a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z",
    shield: "M12 3l7 3v5c0 4.5-3 8.2-7 10-4-1.8-7-5.5-7-10V6z M9 12l2 2 4-4",
    copy: "M9 9h10v10H9z M5 15V5h10",
    search: "M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14z M20 20l-4-4",
    refresh: "M20 11a8 8 0 0 0-14.8-4 M4 4v4h4 M4 13a8 8 0 0 0 14.8 4 M20 20v-4h-4",
    close: "M6 6l12 12 M18 6 6 18",
    stop: "M8 8h8v8H8z M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18z",
    log: "M5 5h14v14H5z M8 9h8 M8 12h8 M8 15h5",
    chevron: "M9 6l6 6-6 6",
  } as const;

  export type IconName = keyof typeof paths;
</script>

<script lang="ts">
  let { name, size = 18 }: { name: IconName; size?: number } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.75"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  <path d={paths[name]} />
</svg>
