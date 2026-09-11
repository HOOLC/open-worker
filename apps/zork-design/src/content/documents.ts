const sources = import.meta.glob<string>("../../docs/*.md", { eager: true, query: "?raw", import: "default" });
export const documents = Object.entries(sources)
  .map(([path, markdown]) => ({
    id: path.split("/").at(-1)!.replace(/\.md$/, ""),
    title: markdown.match(/^#\s+(.+)$/m)?.[1] ?? path,
    markdown,
  }))
  .sort((a, b) => a.id.localeCompare(b.id));
