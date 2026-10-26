use crate::{
    app::{Msg, Page, Playback, Uta},
    model::{Source, Track, ago, time},
};
use aizome_kit::{column, prelude::*, widgets::controls::Button};

pub fn view(app: &Uta) -> Element<Msg> {
    let item = |glyph, label: &str, page| {
        sidebar_item(glyph, label, app.page == page).on_press(Msg::Page(page))
    };
    let mut sidebar = vec![
        row![
            icon(Icon::EighthNotes).size(28.0).role(Role::Accent),
            column![
                text("Uta").size(24.0).semibold(),
                text("YouTube Music").size(11.0).role(Role::Text3)
            ]
            .spacing(2.0)
        ]
        .spacing(12.0)
        .padding([24.0, 16.0])
        .into(),
        caption("LISTEN"),
        item(Icon::Search, "Search", Page::Search).into(),
        item(Icon::List, "Queue", Page::Queue)
            .detail(app.queue.tracks.len().to_string())
            .into(),
        space(16.0),
        caption("LIBRARY"),
        item(Icon::Heart, "Liked songs", Page::Liked).into(),
        item(Icon::Library, "Playlists", Page::Playlists).into(),
        item(Icon::Disc3, "Albums", Page::Albums).into(),
        item(Icon::Clock, "History", Page::History).into(),
        spacer().into(),
    ];
    if app.demo {
        sidebar.push(
            container(
                column![
                    text("DEMO PREVIEW")
                        .size(10.0)
                        .role(Role::Text3)
                        .letter_spacing(1.0),
                    text("Fictional songs; no account or network.")
                        .wrap()
                        .size(12.0)
                        .role(Role::Text2),
                ]
                .spacing(5.0),
            )
            .padding(16.0)
            .into(),
        );
    }
    let account_label = match (app.connected, app.account.is_empty()) {
        (true, false) => app.account.as_str(),
        (true, true) => "YouTube account",
        _ => "Connect account",
    };
    sidebar.extend([
        divider().into(),
        item(Icon::UserRound, account_label, Page::Account)
            .id("account-nav")
            .into(),
        space(8.0),
    ]);
    let mut body = vec![toolbar(vec![
        search_field(&app.query)
            .placeholder("Search songs, albums, playlists, or paste a link…")
            .on_change(Msg::Query)
            .on_submit(Msg::Search)
            .id("search")
            .width(Fill)
            .into(),
        button("Search")
            .on_press(Msg::Search)
            .enabled(!app.account_busy && !app.query.trim().is_empty())
            .into(),
    ])];
    if let Some(error) = &app.error {
        body.push(
            container(
                row![
                    icon(Icon::CircleAlert).role(Role::Danger),
                    text(error.clone()).wrap().role(Role::Danger).width(Fill),
                    icon_button(Icon::X)
                        .on_press(Msg::DismissError)
                        .tooltip("Dismiss error"),
                ]
                .spacing(10.0),
            )
            .padding([10.0, 20.0])
            .fill(Tone::Raised)
            .into(),
        );
    }
    body.push(if app.page == Page::Account {
        account(app)
    } else {
        content(app)
    });
    column![
        sidebar_layout(column(sidebar).height(Fill), column(body).height(Fill))
            .first_size(200.0)
            .min_sizes(170.0, 560.0)
            .height(Fill),
        player(app),
    ]
    .height(Fill)
    .width(Fill)
    .into()
}

fn empty(glyph: Icon, title: &str, detail: &str, action: Option<Element<Msg>>) -> Element<Msg> {
    let mut items = vec![
        icon(glyph).size(42.0).role(Role::Accent).into(),
        space(14.0),
        text(title.to_owned()).size(24.0).semibold().center().into(),
        text(detail.to_owned())
            .size(13.0)
            .role(Role::Text2)
            .wrap()
            .center()
            .max_width(440.0)
            .into(),
    ];
    if let Some(action) = action {
        items.push(space(8.0));
        items.push(action);
    }
    container(column(items).spacing(12.0).align(Align::Center))
        .center()
        .padding(28.0)
        .width(Fill)
        .height(Fill)
        .into()
}

fn primary_if<Msg>(button: Button<Msg>, primary: bool) -> Button<Msg> {
    if primary { button.primary() } else { button }
}

/// The library's invitation to connect, one click when a browser is signed in.
fn connect_invitation(app: &Uta) -> Element<Msg> {
    let best = app.best_source().filter(|_| !app.demo);
    let detail = match best {
        Some(source) => format!(
            "{} is signed in to YouTube. Connect it to see your liked songs, playlists, albums, and history here.",
            source.label()
        ),
        None => "Connect the browser you use for YouTube Music to see your liked songs, playlists, albums, and history here.".into(),
    };
    let action: Element<Msg> = match best {
        Some(source) => row![
            button(format!("Connect {}", source.label()))
                .primary()
                .on_press(Msg::Connect(source.id.clone()))
                .enabled(!app.account_busy)
                .id("connect-best"),
            button("Other ways…").on_press(Msg::Page(Page::Account)),
        ]
        .spacing(8.0)
        .into(),
        None => button("Connect account")
            .primary()
            .on_press(Msg::Page(Page::Account))
            .into(),
    };
    empty(
        Icon::Library,
        "Bring your library along",
        &detail,
        Some(action),
    )
}

fn count(n: usize, one: &str, many: &str) -> String {
    match n {
        0 => String::new(),
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}

fn content(app: &Uta) -> Element<Msg> {
    if app.page.library() && !app.connected {
        return connect_invitation(app);
    }
    let heading = match app.page {
        Page::Search if !app.searched => "Find your next song".to_string(),
        Page::Search => format!("Results for “{}”", app.result_query),
        _ => app.title.clone(),
    };
    let songs = app.visible_tracks().len();
    let status = if app.busy {
        "Loading…".into()
    } else if !app.collections.is_empty() {
        let albums = app.collections.iter().all(|c| c.kind == "album");
        let (one, many) = if albums {
            ("album", "albums")
        } else {
            ("playlist", "playlists")
        };
        count(app.collections.len(), one, many)
    } else if songs >= crate::service::SONG_LIMIT && app.page != Page::Queue {
        format!("First {songs} songs")
    } else {
        count(songs, "song", "songs")
    };
    let mut head = vec![
        row![
            text(heading).size(24.0).semibold().width(Fill),
            text(status).role(Role::Text3)
        ]
        .spacing(16.0)
        .into(),
    ];
    match app.page {
        Page::Search => head.push(
            row![
                segmented(
                    ["Top results", "Songs", "Videos", "Albums", "Playlists"],
                    app.filter
                )
                .on_select(Msg::Filter),
                spacer(),
                text("YOUTUBE MUSIC")
                    .size(10.0)
                    .role(Role::Text3)
                    .letter_spacing(1.0)
            ]
            .into(),
        ),
        Page::Queue => head.push(
            row![
                button("Shuffle")
                    .icon(Icon::Shuffle)
                    .on_press(Msg::Shuffle)
                    .enabled(songs > 1),
                button(app.queue.repeat.label())
                    .icon(Icon::Repeat)
                    .on_press(Msg::Repeat),
                button(if app.radio_busy {
                    "Starting radio…"
                } else {
                    "Start radio"
                })
                .icon(Icon::RadioTower)
                .on_press(Msg::StartRadio(None))
                .enabled(app.queue.track().is_some() && !app.radio_busy && !app.account_busy)
                .tooltip("Fill the queue with songs like the one playing"),
                spacer(),
                button("Clear queue")
                    .on_press(Msg::ClearQueue)
                    .enabled(songs > 0),
            ]
            .spacing(8.0)
            .into(),
        ),
        Page::Collection | Page::Liked | Page::History if app.collections.is_empty() => head.push(
            row![
                button("Play all")
                    .icon(Icon::Play)
                    .primary()
                    .on_press(Msg::Play(0))
                    .enabled(songs > 0 && !app.account_busy),
                button("Start radio")
                    .icon(Icon::RadioTower)
                    .on_press(Msg::StartRadio(app.selected.or(Some(0))))
                    .enabled(songs > 0 && !app.radio_busy && !app.account_busy)
                    .tooltip("A radio from the selected song, or the first"),
            ]
            .spacing(8.0)
            .into(),
        ),
        _ => {}
    }
    let body = if app.busy {
        empty(
            Icon::Headphones,
            "Finding your music…",
            "You can keep listening while this loads.",
            Some(progress_indeterminate().width(180.0).into()),
        )
    } else if !app.collections.is_empty() {
        let rows = app
            .collections
            .iter()
            .enumerate()
            .map(|(i, c)| {
                row![
                    icon(if c.kind == "album" {
                        Icon::Disc3
                    } else {
                        Icon::Library
                    })
                    .size(28.0)
                    .role(Role::Accent),
                    column![
                        text(c.title.clone()).semibold(),
                        text(c.subtitle.clone()).role(Role::Text3).size(12.0)
                    ]
                    .spacing(5.0)
                    .width(Fill),
                    button("Open").on_press(Msg::OpenCollection(i)),
                ]
                .spacing(16.0)
                .padding([10.0, 12.0])
                .into()
            })
            .collect();
        list(rows)
            .row_height(70.0)
            .on_activate(Msg::OpenCollection)
            .height(Fill)
            .into()
    } else if songs > 0 {
        let tracks = std::rc::Rc::new(app.visible_tracks().to_vec());
        let playing = app
            .queue
            .track()
            .filter(|_| app.playback != Playback::Stopped)
            .map(|t| t.id.clone());
        let queue = app.page == Page::Queue;
        let enabled = !app.account_busy;
        let connected = app.connected;
        let menu_tracks = tracks.clone();
        virtual_list(tracks.len(), 64.0, move |i| {
            song_row(
                i,
                &tracks[i],
                playing.as_deref() == Some(tracks[i].id.as_str()),
                queue,
                enabled,
            )
        })
        .selected(app.selected)
        .on_select(|rows| Msg::Select(rows.first().copied()))
        .on_activate(Msg::Play)
        .context_menu(move |i| {
            song_menu(
                i,
                &menu_tracks[i],
                queue.then_some(menu_tracks.len()),
                connected,
                enabled,
            )
        })
        .id("songs")
        .height(Fill)
        .into()
    } else if app.page == Page::Queue {
        empty(
            Icon::List,
            "Make room for a few favorites",
            "Play a song or add one from search to start your queue.",
            Some(button("Find music").on_press(Msg::FocusSearch).into()),
        )
    } else if !app.searched {
        empty(
            Icon::Headphones,
            "Start with something you love",
            "Find a song, an album, or a playlist, or paste a YouTube Music link. Connect your account whenever you want your own library here.",
            None,
        )
    } else if app.error.is_some() {
        empty(
            Icon::CircleAlert,
            "Couldn’t load this music",
            "Try the request again, or reconnect your account if the session has expired.",
            Some(button("Try again").on_press(Msg::Refresh).into()),
        )
    } else {
        empty(
            Icon::Search,
            "Nothing here yet",
            "Try another search, or refresh after adding music to your library.",
            None,
        )
    };
    column![
        column(head).spacing(16.0).padding([24.0, 24.0, 20.0, 24.0]),
        divider(),
        body
    ]
    .height(Fill)
    .into()
}

fn song_row(
    index: usize,
    track: &Track,
    current: bool,
    queue: bool,
    enabled: bool,
) -> Element<Msg> {
    let number: Element<Msg> = if current {
        icon(Icon::Volume2).role(Role::Accent).width(24.0).into()
    } else {
        text(format!("{:02}", index + 1))
            .mono()
            .size(11.0)
            .role(Role::Text3)
            .width(24.0)
            .into()
    };
    row![
        number,
        column![
            text(track.title.clone()).semibold().role(if current {
                Role::Accent
            } else {
                Role::Text
            }),
            text(track.artists.clone()).size(12.0).role(Role::Text2)
        ]
        .spacing(4.0)
        .width(Fill),
        text(track.album.clone())
            .role(Role::Text3)
            .size(12.0)
            .width(145.0),
        // Search's top results don't always know a length; playing does.
        text(if track.duration > 0.0 {
            time(track.duration)
        } else {
            String::new()
        })
        .mono()
        .size(11.0)
        .role(Role::Text3)
        .width(48.0),
        icon_button(Icon::Play)
            .on_press(Msg::Play(index))
            .enabled(enabled)
            .tooltip("Play song")
            .id(format!("play-{index}")),
        if queue {
            icon_button(Icon::X)
                .on_press(Msg::Remove(index))
                .tooltip("Remove from queue")
        } else {
            icon_button(Icon::Plus)
                .on_press(Msg::Enqueue(index))
                .tooltip("Add to queue")
        },
    ]
    .spacing(10.0)
    .padding([8.0, 16.0])
    .into()
}

/// A song's right-click menu; `queue` is the queue's length on its own page.
fn song_menu(
    index: usize,
    track: &Track,
    queue: Option<usize>,
    connected: bool,
    enabled: bool,
) -> Menu<Msg> {
    let mut menu = Menu::new("")
        .item(MenuItem::new("Play", Msg::Play(index)).enabled(enabled))
        .item(MenuItem::new("Play Next", Msg::PlayNext(index)));
    if queue.is_none() {
        menu = menu.item(MenuItem::new("Add to Queue", Msg::Enqueue(index)));
    }
    menu = menu
        .item(MenuItem::new("Start Radio", Msg::StartRadio(Some(index))).enabled(enabled))
        .separator()
        .item(
            MenuItem::new(
                if track.liked { "Unlike" } else { "Like" },
                Msg::Like(track.id.clone(), !track.liked),
            )
            .enabled(connected && enabled),
        )
        .item(MenuItem::new(
            "Copy Song Link",
            Msg::CopyLink(track.id.clone()),
        ));
    if let Some(len) = queue {
        menu = menu
            .separator()
            .item(
                MenuItem::new("Move Up", Msg::Move(index, index.saturating_sub(1)))
                    .enabled(index > 0),
            )
            .item(MenuItem::new("Move Down", Msg::Move(index, index + 1)).enabled(index + 1 < len))
            .item(MenuItem::new("Remove from Queue", Msg::Remove(index)));
    }
    menu
}

fn account(app: &Uta) -> Element<Msg> {
    let busy = app.account_busy;
    let live = !app.demo;
    let mut items: Vec<Element<Msg>> = Vec::new();
    if app.connected {
        items.push(text("Your account").size(28.0).semibold().into());
        let detail = if app.source.is_empty() {
            "Your library and playback share this session.".to_string()
        } else {
            format!(
                "Connected through {}. Your library and playback share this session.",
                app.source
            )
        };
        items.push(
            container(
                row![
                    icon(Icon::CircleCheck).role(Role::Success).size(24.0),
                    column![
                        text(if app.account.is_empty() {
                            "YouTube account".into()
                        } else {
                            app.account.clone()
                        })
                        .semibold(),
                        text(detail).wrap().size(12.0).role(Role::Text2)
                    ]
                    .spacing(5.0)
                    .width(Fill),
                    button("Disconnect")
                        .on_press(Msg::Disconnect)
                        .enabled(!busy)
                        .id("disconnect"),
                ]
                .spacing(12.0),
            )
            .fill(Tone::Raised)
            .border(Edge::Line)
            .padding(16.0)
            .into(),
        );
        if !app.source_id.is_empty() {
            items.push(
                row![
                    text("If YouTube signs Uta out, reconnect to bring in that browser’s current sign-in.")
                        .wrap()
                        .size(12.0)
                        .role(Role::Text2)
                        .width(Fill),
                    button("Reconnect")
                        .icon(Icon::RefreshCw)
                        .on_press(Msg::Connect(app.source_id.clone()))
                        .enabled(!busy && live)
                        .id("reconnect"),
                ]
                .spacing(16.0)
                .into(),
            );
        }
    } else {
        items.push(text("Connect YouTube Music").size(28.0).semibold().into());
        items.push(
            text("Uta uses the sign-in from your web browser, whichever one you use. There’s no password to type and no API key to make.")
                .wrap()
                .role(Role::Text2)
                .into(),
        );
    }
    if busy && app.auth_generation > 0 {
        items.push(
            row![
                progress_indeterminate().width(120.0),
                text("Connecting to YouTube…").role(Role::Text2),
            ]
            .spacing(12.0)
            .into(),
        );
    }

    let signed: Vec<&Source> = app
        .sources
        .iter()
        .filter(|s| s.signed_in && !(app.connected && s.id == app.source_id))
        .collect();
    if !signed.is_empty() {
        let rows = signed
            .iter()
            .enumerate()
            .map(|(i, source)| {
                let action = primary_if(
                    button(if app.connected { "Switch" } else { "Connect" }),
                    i == 0 && !app.connected,
                )
                .on_press(Msg::Connect(source.id.clone()))
                .enabled(!busy && live)
                .id(format!("connect-{i}"));
                let description = if source.used > 0.0 {
                    format!("Signed in, active {}", ago(source.used, app.sources_now))
                } else {
                    "Signed in".into()
                };
                form_row(source.label(), action)
                    .icon(Icon::Globe)
                    .description(description)
                    .into()
            })
            .collect();
        let title = if app.connected {
            "Other browsers signed in"
        } else {
            "Signed in to YouTube"
        };
        items.push(form_section(title, rows));
    } else if !app.sources_checked && app.discovering {
        items.push(
            text("Looking for browsers signed in to YouTube…")
                .size(12.0)
                .role(Role::Text3)
                .into(),
        );
    }
    if !app.connected {
        items.push(if app.signing_in {
            container(
                row![
                    progress_indeterminate().width(96.0),
                    column![
                        text("Waiting for you to sign in").semibold(),
                        text("Sign in at music.youtube.com in your browser. Uta connects as soon as the browser saves the sign-in, which can take half a minute.")
                            .wrap()
                            .size(12.0)
                            .role(Role::Text2),
                    ]
                    .spacing(4.0)
                    .width(Fill),
                    button("Stop waiting").on_press(Msg::StopWaiting),
                ]
                .spacing(16.0),
            )
            .fill(Tone::Raised)
            .border(Edge::Line)
            .padding(16.0)
            .into()
        } else {
            row![
                text(if signed.is_empty() {
                    "Sign in to YouTube Music in any browser on this computer, and Uta will find it."
                } else {
                    "Or sign in with another browser."
                })
                .wrap()
                .role(Role::Text2)
                .width(Fill),
                primary_if(button("Sign in with your browser"), signed.is_empty())
                    .icon(Icon::ExternalLink)
                    .on_press(Msg::SignIn)
                    .enabled(!busy && live)
                    .id("sign-in"),
            ]
            .spacing(16.0)
            .into()
        });
    }
    // What else was looked at, while there is something to connect.
    let unsigned: Vec<String> = app
        .sources
        .iter()
        .filter(|s| !s.signed_in)
        .map(Source::label)
        .collect();
    let checked = if app.connected {
        None
    } else if !unsigned.is_empty() {
        Some(format!(
            "Also checked, not signed in: {}.",
            unsigned.join(", ")
        ))
    } else if app.sources_checked && app.sources.is_empty() {
        Some("Uta found no browser profiles in the usual places. Use one of the ways below.".into())
    } else {
        None
    };
    if let Some(checked) = checked {
        items.push(text(checked).wrap().size(12.0).role(Role::Text3).into());
    }
    if !app.account_note.is_empty() {
        items.push(
            text(app.account_note.clone())
                .wrap()
                .role(Role::Warning)
                .size(12.0)
                .into(),
        );
    }
    items.push(form_section(
        "Other ways to connect",
        vec![
            form_row(
                "Copy from your browser",
                button("Connect from clipboard")
                    .on_press(Msg::ConnectClipboard)
                    .enabled(!busy && live)
                    .id("connect-clipboard"),
            )
            .icon(Icon::ClipboardPaste)
            .description("On music.youtube.com, open the developer tools’ Network tab and copy any request as cURL, or its request headers. A cookies.txt export works too.")
            .into(),
            form_row(
                "Choose a profile folder",
                button("Choose folder…")
                    .on_press(Msg::ChooseFolder)
                    .enabled(!busy && live)
                    .id("choose-folder"),
            )
            .icon(Icon::FolderOpen)
            .description("For a browser Uta doesn’t know, or a profile kept somewhere else.")
            .into(),
        ],
    ));
    items.extend([
        text("Uta keeps only YouTube’s cookies, in your desktop keyring, and reads a browser only when you connect it. Disconnect removes them.")
            .wrap()
            .size(12.0)
            .role(Role::Text2)
            .into(),
        space(4.0),
        divider().into(),
        text("Listening with Premium").semibold().into(),
        text("Uta requests audio with your connected account and chooses the best format YouTube makes available. The player shows the actual codec and bitrate. Availability depends on the song and YouTube’s current playback requirements.")
            .wrap()
            .size(12.0)
            .role(Role::Text2)
            .into(),
    ]);
    scroll(column(items).spacing(16.0).padding(28.0).max_width(760.0))
        .height(Fill)
        .into()
}

fn player(app: &Uta) -> Element<Msg> {
    let track = app.queue.track();
    let cover: Element<Msg> = if let Some(art) = &app.artwork {
        image(art.clone())
            .fit(Fit::Cover)
            .width(56.0)
            .height(56.0)
            .into()
    } else {
        container(icon(Icon::Disc3).size(30.0).role(Role::Accent))
            .center()
            .width(56.0)
            .height(56.0)
            .fill(Tone::Sunk)
            .border(Edge::Line)
            .into()
    };
    let glyph = match app.playback {
        Playback::Playing => Icon::Pause,
        Playback::Resolving => Icon::Square,
        _ => Icon::Play,
    };
    let label = match app.playback {
        Playback::Playing => "Pause",
        Playback::Resolving => "Cancel loading",
        _ => "Play",
    };
    let mut detail = app.stream_label.clone();
    if app.playback == Playback::Resolving {
        detail = "Opening audio…".into();
    }
    if track.is_none() {
        detail = "Choose a song to begin".into();
    }
    let mut buttons = vec![
        icon_button(Icon::SkipBack)
            .on_press(Msg::Previous)
            .enabled(track.is_some())
            .tooltip("Previous song")
            .into(),
        button(label)
            .icon(glyph)
            .primary()
            .on_press(Msg::Toggle)
            .enabled(!app.account_busy && (track.is_some() || !app.tracks.is_empty()))
            .id("toggle-play")
            .into(),
        icon_button(Icon::SkipForward)
            .on_press(Msg::Next)
            .enabled(track.is_some())
            .tooltip("Next song")
            .into(),
    ];
    if let Some(track) = track {
        buttons.push(
            icon_button(Icon::Heart)
                .selected(track.liked)
                .on_press(Msg::Like(track.id.clone(), !track.liked))
                .enabled(app.connected && !app.account_busy && !app.rating.contains(&track.id))
                .tooltip("Like / unlike song")
                .into(),
        );
        buttons.push(
            icon_button(Icon::RadioTower)
                .on_press(Msg::StartRadio(None))
                .enabled(!app.account_busy && !app.radio_busy)
                .tooltip("Start radio from this song")
                .into(),
        );
    }
    container(
        column![
            row![
                cover,
                column![
                    text(
                        track
                            .map(|t| t.title.as_str())
                            .unwrap_or("Nothing playing")
                            .to_owned()
                    )
                    .semibold(),
                    text(
                        track
                            .map(|t| t.artists.as_str())
                            .unwrap_or("Uta · YouTube Music")
                            .to_owned()
                    )
                    .size(12.0)
                    .role(Role::Text2),
                    text(detail).size(10.0).role(Role::Text3)
                ]
                .spacing(5.0)
                .width(Fill),
                row(buttons).spacing(8.0),
                icon(Icon::Volume2).role(Role::Text3),
                slider(app.volume, 0.0..=100.0)
                    .on_change(Msg::Volume)
                    .width(100.0)
                    .label("Volume"),
            ]
            .spacing(14.0),
            row![
                text(time(app.position))
                    .mono()
                    .size(10.0)
                    .role(Role::Text3)
                    .width(42.0),
                slider(app.position, 0.0..=app.duration.max(1.0))
                    .on_change(Msg::Seek)
                    .width(Fill)
                    .label("Playback position"),
                text(time(app.duration))
                    .mono()
                    .size(10.0)
                    .role(Role::Text3)
                    .width(48.0)
            ]
            .spacing(8.0),
        ]
        .spacing(10.0),
    )
    .fill(Tone::Chrome)
    .edges(Edge::Line, Sides::TOP)
    .padding([14.0, 20.0])
    .into()
}
