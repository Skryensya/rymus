// Who made a song or an album (i, or the row menu), and its link to share: what YouTube Music says, in a panel over
// the lists that any key closes. An album's credits are those of all its songs together.
use crate::i18n::tr;
use super::*;

// What the panel shows.
pub struct Credits {
    pub item: Item,
    pub about: Vec<String>,                // an album's year, length and description
    pub roles: crate::yt::Roles,          // ("Produced by", names)
}

// Only what YouTube Music has can be shared or credited: not your own files, nor albums known only by name (Stats).
fn on_youtube(it: &Item) -> bool {
    matches!(it.kind, Kind::Track | Kind::Album) && !it.id.is_empty() && !it.id.starts_with(crate::disk::OWN)
}
pub fn share_url(it: &Item) -> String {
    if it.kind == Kind::Album { format!("https://music.youtube.com/browse/{}", it.id) } else { track_url(it) }
}

impl App {
    // `it`, or what plays when there is no row to take it from.
    fn credit_target(&self, it: Option<Item>) -> Option<Item> {
        it.or_else(|| self.now().cloned())
    }
    pub(super) fn show_credits(&mut self, it: Option<Item>) {
        let Some(it) = self.credit_target(it) else { return self.set_note(tr!("Nothing to show the credits of").into()) };
        if !on_youtube(&it) {
            return self.set_note(tr!("No credits: it is not on YouTube Music").into());
        }
        let yt = self.yt.clone();
        self.run_busy(tr!("Looking up the credits…").into(), move || {
            let (about, roles) = if it.kind == Kind::Album { yt.album_credits(&it)? } else { (vec![], yt.credits(&it)?) };
            Ok(Box::new(move |app: &mut App| {
                app.panel_scroll = 0;
                app.credits = Some(Credits { item: it, about, roles });
            }))
        });
    }
    // Copies the link to the clipboard; where there is no clipboard tool it is shown to copy by hand.
    pub(super) fn share(&mut self, it: Option<Item>) {
        let Some(it) = self.credit_target(it) else { return self.set_note(tr!("Nothing to share").into()) };
        if !on_youtube(&it) {
            return self.set_note(tr!("Nothing to share: it is not on YouTube Music").into());
        }
        let url = share_url(&it);
        if crate::platform::copy(&url) {
            self.set_note(tr!("Link copied · {url}", url = url));
        } else {
            self.set_note(tr!("The link: {url}", url = url));
        }
    }
}
