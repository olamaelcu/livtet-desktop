import { error } from '@sveltejs/kit'

/** ULIDs are 26 Crockford base32 characters (I, L, O, U excluded). */
const ULID = /^[0-9a-hjkmnp-tv-z]{26}$/i

export const load = ({ params }: { params: { catalogId: string } }) => {
  if (!ULID.test(params.catalogId)) error(404, 'Unknown catalog')
  return { pageTitle: 'Catalog', catalogId: params.catalogId }
}
