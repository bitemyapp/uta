use crate::{
    app::{Msg, Page, Playback, Uta},
    model::{Track, time},
};
use aizome_kit::{column, prelude::*};

pub fn view(app: &Uta) -> Element<Msg> {
    let sidebar = column![
        row![
            icon(Icon::Headphones).size(28.0).role(Role::Accent),
            column![
                text("Uta").size(24.0).semibold(),
                text("YouTube Music").size(11.0).role(Role::Text3)
            ]
            .spacing(2.0)
        ]
        .spacing(12.0)
        .padding([24.0, 16.0]),
        caption("LISTEN"),
        sidebar_item(Icon::Search, "Search", app.page == Page::Search)
            .on_press(Msg::Page(Page::Search)),
        sidebar_item(Icon::Heart, "Liked songs", app.page == Page::Liked)
            .on_press(Msg::Page(Page::Liked)),
        sidebar_item(Icon::Library, "Playlists", app.page == Page::Playlists)
            .on_press(Msg::Page(Page::Playlists)),
        space(16.0),
        caption("PLAYBACK"),
        sidebar_item(Icon::List, "Queue", app.page == Page::Queue)
            .detail(app.queue.tracks.len().to_string())
            .on_press(Msg::Page(Page::Queue)),
        spacer(),
        container(
            column![
                text(if app.demo {
                    "DEMO PREVIEW"
                } else {
                    "MADE FOR AIZOME"
                })
                .size(10.0)
                .role(Role::Text3)
                .letter_spacing(1.0),
                text("A place for your music.").size(12.0).role(Role::Text2),
            ]
            .spacing(5.0)
        )
        .padding(16.0),
        divider(),
        sidebar_item(
            Icon::UserRound,
            if app.connected {
                "Connected account"
            } else {
                "Connect account"
            },
            app.page == Page::Account
        )
        .on_press(Msg::Page(Page::Account))
        .id("account-nav"),
        space(8.0),
    ]
    .height(Fill);
    let mut body = vec![toolbar(vec![
        search_field(&app.query)
            .placeholder("Search songs, albums, playlists…")
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
        sidebar_layout(sidebar, column(body).height(Fill))
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

fn content(app: &Uta) -> Element<Msg> {
    if matches!(app.page, Page::Liked | Page::Playlists) && !app.connected {
        return empty(
            Icon::Library,
            "Bring your library along",
            "Connect the browser you use for YouTube Music to see your liked songs and playlists here.",
            Some(
                button("Connect account")
                    .primary()
                    .on_press(Msg::Page(Page::Account))
                    .into(),
            ),
        );
    }
    let heading = match app.page {
        Page::Search if !app.searched => "Find your next song".to_string(),
        Page::Search => format!("Results for “{}”", app.result_query),
        _ => app.title.clone(),
    };
    let count = if app.collections.is_empty() {
        app.visible_tracks().len()
    } else {
        app.collections.len()
    };
    let mut head = vec![
        row![
            text(heading).size(24.0).semibold().width(Fill),
            text(if app.busy {
                "Searching…".into()
            } else if count > 0 {
                format!("{count} items")
            } else {
                String::new()
            })
            .role(Role::Text3)
        ]
        .spacing(16.0)
        .into(),
    ];
    if app.page == Page::Search {
        head.push(
            row![
                segmented(["Songs", "Albums", "Playlists"], app.filter).on_select(Msg::Filter),
                spacer(),
                text("YOUTUBE MUSIC")
                    .size(10.0)
                    .role(Role::Text3)
                    .letter_spacing(1.0)
            ]
            .into(),
        );
    } else if app.page == Page::Queue {
        head.push(
            row![
                button("Shuffle")
                    .icon(Icon::Shuffle)
                    .on_press(Msg::Shuffle)
                    .enabled(count > 1),
                button(app.queue.repeat.label())
                    .icon(Icon::Repeat)
                    .on_press(Msg::Repeat),
                spacer(),
                button("Clear queue")
                    .on_press(Msg::ClearQueue)
                    .enabled(count > 0),
            ]
            .spacing(8.0)
            .into(),
        );
    } else if app.page == Page::Collection || app.page == Page::Liked {
        head.push(
            row![
                button("Play all")
                    .icon(Icon::Play)
                    .primary()
                    .on_press(Msg::Play(0))
                    .enabled(count > 0 && !app.account_busy),
                spacer(),
                text("Up to 200 songs").size(11.0).role(Role::Text3),
            ]
            .into(),
        );
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
    } else if !app.visible_tracks().is_empty() {
        let tracks = app.visible_tracks().to_vec();
        let playing = app
            .queue
            .track()
            .filter(|_| app.playback != Playback::Stopped)
            .map(|t| t.id.clone());
        let queue = app.page == Page::Queue;
        let enabled = !app.account_busy;
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
            "Find a song, an album, or a playlist. Connect your account whenever you want your own library here.",
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
        text(time(track.duration))
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

fn account(app: &Uta) -> Element<Msg> {
    let mut items: Vec<Element<Msg>> = vec![
        text("Your music, with you").size(28.0).semibold().into(),
        text(
            "Connect your YouTube Music account using the browser where you are already signed in.",
        )
        .wrap()
        .role(Role::Text2)
        .into(),
        space(8.0),
    ];
    if app.connected {
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
                        text("Your library and playback share this session.")
                            .size(12.0)
                            .role(Role::Text2)
                    ]
                    .spacing(5.0)
                    .width(Fill),
                    button("Disconnect")
                        .on_press(Msg::Disconnect)
                        .enabled(!app.account_busy)
                        .id("disconnect"),
                ]
                .spacing(12.0),
            )
            .fill(Tone::Raised)
            .border(Edge::Line)
            .padding(16.0)
            .into(),
        );
    }
    items.extend([
        text("1. Sign in with your browser").semibold().into(),
        row![text("Open YouTube Music and sign in to the account you want to use.").wrap().width(Fill).role(Role::Text2),
            button("Open YouTube Music").icon(Icon::ExternalLink).on_press(Msg::OpenLogin)
        ].spacing(16.0).into(), divider().into(),
        text("2. Connect that browser").semibold().into(),
        row![text("Browser").width(100.0), dropdown(["Firefox", "Chromium", "Chrome", "Brave", "Edge", "Vivaldi", "Opera"], app.browser)
            .on_select(Msg::Browser).enabled(!app.account_busy).width(Fill)].spacing(12.0).into(),
        row![text("Profile").width(100.0), text_field(&app.profile).placeholder("Default profile (or enter its directory)")
            .on_change(Msg::Profile).enabled(!app.account_busy).width(Fill)].spacing(12.0).into(),
        text("Uta imports only YouTube cookies and stores the session in your desktop keyring. No API credentials are needed. Reconnect here when the session expires.")
            .wrap().size(12.0).role(Role::Text2).into(),
        row![button(if app.account_busy { "Connecting…" } else if app.connected { "Reconnect browser" } else { "Connect browser" })
            .primary().on_press(Msg::Connect).enabled(!app.account_busy && !app.demo).id("connect"), spacer()].into(),
    ]);
    if !app.account_note.is_empty() {
        items.push(
            text(app.account_note.clone())
                .wrap()
                .role(Role::Warning)
                .size(12.0)
                .into(),
        );
    }
    items.extend([space(4.0), divider().into(), text("Listening with Premium").semibold().into(),
        text("Uta requests audio with your connected account and chooses the best format YouTube makes available. The player shows the actual codec and bitrate. Availability depends on the song and YouTube’s current playback requirements.")
            .wrap().size(12.0).role(Role::Text2).into()]);
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
