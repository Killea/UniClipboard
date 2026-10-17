/**
 * Root-relative path helpers for the remote-share wire surface.
 *
 * Peer share paths are always '/'-joined, root-relative, and carry no leading
 * or trailing slash ('' is the root's top level). Host-side separators are
 * translated at the confinement layer; the UI only ever builds these.
 */

/** Join a root-relative dir and a single segment. */
export const relJoin = (dir: string, segment: string): string =>
  dir === '' ? segment : `${dir}/${segment}`

/** Last segment of a root-relative path ('' for the root). */
export const relBaseName = (rel: string): string =>
  rel === '' ? '' : rel.slice(rel.lastIndexOf('/') + 1)

/** Parent dir of a root-relative path; '' for top-level or root. */
export const relDirName = (rel: string): string => {
  const i = rel.lastIndexOf('/')
  return i < 0 ? '' : rel.slice(0, i)
}

/** Depth in segments — '' is depth 0. Bounded by PEER_SHARE_MAX_DEPTH in UI. */
export const relDepth = (rel: string): number => (rel === '' ? 0 : rel.split('/').length)

/** Split a root-relative path into ordered segments for breadcrumbs. */
export const relSegments = (rel: string): string[] => (rel === '' ? [] : rel.split('/'))
