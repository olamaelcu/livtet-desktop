import { error } from '@sveltejs/kit'

/** ULIDs are 26 Crockford base32 characters (I, L, O, U excluded). */
const ULID = /^[0-9a-hjkmnp-tv-z]{26}$/i

export const load = ({ params }: { params: { editionId: string } }) => {
  if (!ULID.test(params.editionId)) error(404, 'Unknown edition')
  return { pageTitle: 'Reader', editionId: params.editionId }
}
