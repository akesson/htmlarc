use std::fmt::Display;

use strum_macros::{EnumString, FromRepr, IntoStaticStr};

#[allow(non_camel_case_types, clippy::upper_case_acronyms)]
#[repr(u8)]
#[derive(
    FromRepr, EnumString, IntoStaticStr, Debug, PartialEq, Eq, Clone, Copy, PartialOrd, Ord, Hash,
)]
#[strum(ascii_case_insensitive, serialize_all = "kebab-case")]
pub enum HtmlAttr {
    sys_deleted = 0,
    accesskey, //Defines a shortcut key to activate/focus an element.
    action,
    align, //Specifies the alignment of the element according to the surrounding context.
    alt,   //Provides alternative text for an image, if the image cannot be displayed.
    aria_expanded,
    aria_hidden,
    aria_label,
    aria_labelledby,
    aria_haspopup,
    aria_controls,
    aria_pressed,
    #[strum(serialize = "async")]
    async_, //Indicates that the script should be executed asynchronously as soon as it is available.
    autocapitalize,
    autocomplete, //Specifies whether a form or an input field should have autocomplete enabled.
    autofocus,    //Sets the focus on a particular element when the page loads.
    autoplay,     //Allows a media element to start playing automatically on page load.
    bgcolor,      //Sets the background color of an element.
    border,       //Defines the width of the border of a frame.
    cellspacing,
    cellpadding,
    charset,         //Specifies the character encoding for the linked resource.
    checked,         //Indicates whether a checkbox or radio button is checked by default.
    clear,           // deprecated, but still used in MW.
    cite,            //Defines the URL of a quote’s source.
    class, //Assigns one or more class names to an element, which can be used by CSS and JavaScript.
    color, //Specifies the color of the text.
    cols,  //Defines the number of columns in a textarea.
    colspan, //Specifies the number of columns a cell should span.
    content, //Gives the value associated with the http-equiv or name attribute.
    contenteditable, //Indicates whether the content of an element is editable or not.
    controls, //Specifies that audio/video controls should be displayed.
    coords, //Defines the coordinates of an area in an image map.
    // note data attributes are dropped
    datetime, //Specifies the date and time.
    decoding,
    default, //Indicates that the track should be enabled if the user’s preferences do not indicate otherwise.
    defer,   //Specifies that the script is executed when the page has finished parsing.
    dir,     //Defines the text direction for the content in an element.
    dirname, //Allows to submit the directionality of the element.
    disabled, //Indicates that the user cannot interact with the element.
    download, //Specifies that the target will be downloaded when a user clicks on the hyperlink.
    draggable, //Defines whether an element can be dragged.
    enctype, //Specifies how form data should be encoded when submitting to the server.
    #[strum(serialize = "for")]
    for_, //Specifies the association between the label and form element.
    form,    //Specifies the form the element belongs to.
    formaction, //Defines the action of the form, where to send form-data on submission.
    frame,
    headers,    //Specifies the headers associated with a table cell.
    height,     //Sets the height of an element.
    hidden,     //Indicates that the element is not yet, or no longer, relevant.
    high,       //Specifies the range that is considered to be a high value in a gauge/meter.
    href,       //Specifies the URL of a link.
    hreflang,   //Indicates the language of the linked resource.
    html,       // only used in the DOCTYPE tag
    http_equiv, //Provides an HTTP header for the information/value of the content attribute.
    id,         //Defines a unique identifier for an element.
    ismap,      //Indicates that the image is part of a server-side image map.
    kind,       //Specifies the kind of text track in a media element.
    label,      //Specifies the title of the text track.
    lang,       //Defines the language of an element’s content.
    list, //Refers to a datalist element that contains pre-defined options for an input element.
    loading,
    #[strum(serialize = "loop")]
    loop_, //Specifies that the audio/video will start over again, every time it is finished.
    low,              //Indicates the range that is considered to be a low value in a gauge/meter.
    max,              //Defines the maximum value for an element.
    maxlength,        //Specifies the maximum number of characters allowed in an element.
    media,            //Specifies what media/device the linked document is optimized for.
    method,           //Defines the HTTP method for sending form-data.
    min,              //Defines the minimum value for an element.
    multiple,         //Indicates that multiple options can be selected in a list.
    muted,            //Specifies that the audio output of the video should be muted.
    name,             //Sets the name of an element.
    novalidate,       //Indicates that the form should not be validated when submitted.
    onabort,          //Script to be run on aborting an operation.
    onafterprint,     //Script to be run after the document is printed.
    onbeforeprint,    //Script to be run before the document is printed.
    onbeforeunload,   //Script to be run when the document is about to be unloaded.
    onblur,           //Script to be run when the element loses focus.
    oncanplay, //Script to be run when a media can start play, but might has to stop for buffering.
    onchange,  //Script to be run when the value of the element changes.
    onclick,   //Script to be run when the element is clicked.
    oncontextmenu, //Script to be run when a context menu is triggered.
    oncopy,    //Script to be run when the content of the element is being copied.
    oncuechange, //Script to be run when the cue changes in a track element.
    oncut,     //Script to be run when the content of the element is being cut.
    ondblclick, //Script to be run when the element is double-clicked.
    ondrag,    //Script to be run when the element is being dragged.
    ondragend, //Script to be run at the end of a drag operation.
    ondragenter, //Script to be run when the element has been dragged to a valid drop target.
    ondragleave, //Script to be run when the element leaves a valid drop target.
    ondragover, //Script to be run when the element is being dragged over a valid drop target.
    ondragstart, //Script to be run at the start of a drag operation.
    ondrop,    //Script to be run when the dragged element is being dropped.
    ondurationchange, //Script to be run when the duration of the media changes.
    onemptied, //Script to be run when something bad happens and the file is suddenly unavailable.
    onended,   //Script to be run when the media has reached the end.
    onerror,   //Script to be run when an error occurs.
    onfocus,   //Script to be run when the element gets focus.
    onhashchange, //Script to be run when there has been changes to the anchor part of the URL.
    oninput,   //Script to be run when the element gets user input.
    oninvalid, //Script to be run when the element is invalid.
    onkeydown, //Script to be run when a user is pressing a key.
    onkeypress, //Script to be run when a user presses a key.
    onkeyup,   //Script to be run when a user releases a key.
    onload,    //Script to be run when the element has finished loading.
    onloadeddata, //Script to be run when media data is loaded.
    onloadedmetadata, //Script to be run when the metadata of the media has been loaded.
    onloadstart, //Script to be run just as the file begins to load before anything is actually loaded.
    onmessage,   //Script to be run when a message is received through the event source.
    onmousedown, //Script to be run when a mouse button is pressed down on an element.
    onmouseenter, //Script to be run when the pointer is moved onto an element.
    onmouseleave, //Script to be run when the pointer is moved out of an element.
    onmousemove, //Script to be run as long as the pointer is moving over an element.
    onmouseout,  //Script to be run when a user moves the mouse pointer out of an element.
    onmouseover, //Script to be run when the pointer is moved onto an element.
    onmouseup,   //Script to be run when a mouse button is released over an element.
    onmousewheel, //Script to be run when a mouse wheel is being scrolled over an element.
    onoffline,   //Script to be run when the browser starts to work offline.
    ononline,    //Script to be run when the browser starts to work online.
    onpagehide,  //Script to be run when a user navigates away from a page.
    onpageshow,  //Script to be run when a user navigates to a page.
    onpaste,     //Script to be run when the user pastes some content in an element.
    onpause,     //Script to be run when the media is paused either by the user or programmatically.
    onplay,      //Script to be run when the media has started playing.
    onplaying, //Script to be run when the media is playing after having been paused or stopped for buffering.
    onpopstate, //Script to be run when the window’s history changes.
    onprogress, //Script to be run when the browser is in the process of getting the media data.
    onratechange, //Script to be run each time the playback rate changes (like when a user switches to a slow motion or fast forward mode).
    onreset,      //Script to be run when a form is reset.
    onresize,     //Script to be run when the window gets resized.
    onscroll,     //Script to be run when an element’s scrollbar is being scrolled.
    onsearch, //Script to be run when the user writes something in a search field (for <input=”search”>).
    onseeked, //Script to be run when the seeking attribute is set to false indicating that seeking has ended.
    onseeking, //Script to be run when the seeking attribute is set to true indicating that seeking is active.
    onselect,  //Script to be run when the element gets selected.
    onstalled, //Script to be run when the browser is trying to get media data, but data is not available.
    onstorage, //Script to be run when a Web Storage area is updated.
    onsubmit,  //Script to be run when a form is submitted.
    onsuspend, //Script to be run when the browser is intentionally not getting media data.
    ontimeupdate, //Script to be run when the playing position has changed (like when the user fast forwards to a different point in the media).
    ontoggle,     //Script to be run when the user opens or closes the <details> element.
    onunload, //Script to be run when a page has unloaded (or the browser window has been closed).
    onvolumechange, //Script to be run each time the volume of a video/audio has been changed.
    onwaiting, //Script to be run when the media has paused but is expected to resume (like when the media pauses to buffer more data).
    onwheel,   //Script to be run when a wheel button of a pointing device is rotated.
    open,      //Specifies whether the details will be visible or not.
    optimum,   //Specifies what would be a optimal value in a gauge/meter.
    pattern,   //Defines a pattern (regular expression) the input field’s value is checked against.
    placeholder, //Provides a hint to the user of what can be entered in the input field.
    poster, //Specifies an image to be shown while the video is downloading, or until the user hits the play button.
    preload, //Specifies if and how the author thinks the video/audio should be loaded when the page loads.
    property,
    readonly, //Specifies that the input field is read-only.
    rel,      //Specifies the relationship between the current document and the linked document.
    required, //Specifies that the input field must be filled out before submitting the form.
    reversed, //Specifies that the list order should be descending (9,8,7…).
    role,
    rows,       //Specifies the visible number of lines in a text area.
    rowspan,    //Specifies the number of rows a table cell should span.
    sandbox,    //Enables an extra set of restrictions for the content in an iframe.
    scope, //Specifies whether a header cell is a header for a column, row, or group of columns or rows.
    scoped, //Specifies that the styles only apply to this element’s parent element and that element’s child elements.
    selected, //Specifies that an option should be pre-selected when the page loads.
    shape,  //Specifies the shape of the area.
    size, //Specifies the width, in characters (for <input>) or specifies the number of visible options (for <select>).
    sizes, //Specifies the size of the linked resource.
    span, //Specifies the number of columns to span.
    spellcheck, //Specifies whether the element is to have its spelling and grammar checked or not.
    src,  //Specifies the URL of the media file.
    srcdoc, //Specifies the HTML content of the page to show in the iframe.
    srclang, //Specifies the language of the track text data (for <track> in <audio> and <video> elements).
    srcset,  //Specifies the URL of the image to use in different situations.
    start,   //Specifies the start value of an ordered list.
    step,    //Specifies the legal number intervals for an input field.
    style,   //Specifies an inline CSS style for an element.
    tabindex, //Sets the tab order of an element.
    target, //Specifies the target for where to open the linked document or where to submit the form.
    title,  //Specifies extra information about an element (displayed as a tooltip).
    translate, //Specifies whether the content of an element should be translated or not.
    #[strum(serialize = "type")]
    type_, //Specifies the type of element.
    #[strum(serialize = "typeof")]
    typeof_,
    usemap, //Specifies an image as a client-side image map.
    value,  //Specifies the value of the element.
    valign,
    width, //Sets the width of an element.
    wrap,  //Specifies how the text in a text area is to be wrapped when submitted in a form.
    // NON-STANDARD ATTRIBUTES USED BY MEDIAWIKI
    resource,
    rules,
}

impl Display for HtmlAttr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.into())
    }
}

impl HtmlAttr {
    /// By default these attributes are case-sensitive.<br>
    /// https://developer.mozilla.org/en-US/docs/Web/CSS/Attribute_selectors#description
    pub const fn is_case_sensitive(&self) -> bool {
        matches!(
            self,
            HtmlAttr::id
                | HtmlAttr::aria_controls
                | HtmlAttr::aria_expanded
                | HtmlAttr::aria_haspopup
                | HtmlAttr::aria_hidden
                | HtmlAttr::aria_label
                | HtmlAttr::aria_labelledby
                | HtmlAttr::aria_pressed
                | HtmlAttr::role
        )
    }
}
