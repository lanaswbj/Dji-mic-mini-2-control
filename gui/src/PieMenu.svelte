<script>
  import { onMount, onDestroy } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { pieMenuAnswerQuestion, pieMenuClose, pieMenuSelect } from "./lib/api.js";

  // Slot 0-5 order matches `pie_menu_select`'s dispatch in
  // gui/src-tauri/src/pie_menu.rs: voice input, Down, Up, Enter, "/btw ",
  // close-only. Used only for DEFAULT_ITEMS.length/keying below — the six
  // SF Symbol icons themselves are DEFAULT_ICONS, indexed the same way.
  // Overridden entirely by `pendingQuestion` (see ITEMS/ICONS below) while a
  // Claude Code permission-request question is showing instead.
  const DEFAULT_ITEMS = ["voice", "down", "up", "enter", "btw", "close"];

  // Inlined path data from the local SF Symbols set (E:\sf symbol) rather
  // than bundled image assets — each icon is small enough, and there are
  // only six of them, that a plain lookup table keeps everything
  // self-contained in this one component. `fill="currentColor"` (set in the
  // markup below) lets each icon inherit the item's own text color.
  const DEFAULT_ICONS = [
    {
      // microphone.svg
      viewBox: "0 0 14.3359 22.2949",
      paths: [
        "M6.99219 17.168C11.1426 17.168 13.9746 14.3848 13.9746 10.3223L13.9746 8.33984C13.9746 7.92969 13.6523 7.60742 13.2422 7.60742C12.832 7.60742 12.5 7.92969 12.5 8.33984L12.5 10.2637C12.5 13.5938 10.332 15.8008 6.99219 15.8008C3.64258 15.8008 1.47461 13.5938 1.47461 10.2637L1.47461 8.33984C1.47461 7.92969 1.15234 7.60742 0.732422 7.60742C0.322266 7.60742 0 7.92969 0 8.33984L0 10.3223C0 14.3848 2.83203 17.168 6.99219 17.168ZM3.4375 9.9707C3.4375 12.2168 4.88281 13.7988 6.99219 13.7988C9.0918 13.7988 10.5371 12.2168 10.5371 9.9707L10.5371 3.82812C10.5371 1.57227 9.0918 0 6.99219 0C4.88281 0 3.4375 1.57227 3.4375 3.82812ZM4.91211 9.9707L4.91211 3.82812C4.91211 2.38281 5.74219 1.45508 6.99219 1.45508C8.24219 1.45508 9.0625 2.38281 9.0625 3.82812L9.0625 9.9707C9.0625 11.416 8.24219 12.3438 6.99219 12.3438C5.74219 12.3438 4.91211 11.416 4.91211 9.9707ZM2.62695 20.8984L11.3477 20.8984C11.7578 20.8984 12.0898 20.5762 12.0898 20.166C12.0898 19.7559 11.7578 19.4238 11.3477 19.4238L2.62695 19.4238C2.2168 19.4238 1.88477 19.7559 1.88477 20.166C1.88477 20.5762 2.2168 20.8984 2.62695 20.8984ZM6.99219 20.5762C7.40234 20.5762 7.72461 20.2441 7.72461 19.834L7.72461 16.8359C7.72461 16.4258 7.40234 16.0938 6.99219 16.0938C6.58203 16.0938 6.25 16.4258 6.25 16.8359L6.25 19.834C6.25 20.2441 6.58203 20.5762 6.99219 20.5762Z",
      ],
    },
    {
      // arrow.down.svg
      viewBox: "0 0 15.166 18.4473",
      paths: [
        "M7.40234 18.4473C7.64648 18.4473 7.87109 18.3496 8.05664 18.1543L14.541 11.6602C14.7266 11.4648 14.8047 11.2598 14.8047 11.0254C14.8047 10.5469 14.4531 10.1758 13.9648 10.1758C13.7305 10.1758 13.5059 10.2539 13.3496 10.4102L11.123 12.5977L7.39258 16.6797L3.68164 12.5977L1.45508 10.4102C1.30859 10.2539 1.07422 10.1758 0.839844 10.1758C0.351562 10.1758 0 10.5469 0 11.0254C0 11.2598 0.0878906 11.4648 0.273438 11.6602L6.74805 18.1543C6.93359 18.3496 7.1582 18.4473 7.40234 18.4473ZM7.40234 17.4023C7.86133 17.4023 8.17383 17.0898 8.17383 16.6309L8.27148 13.7207L8.27148 0.859375C8.27148 0.351562 7.91016 0 7.40234 0C6.89453 0 6.5332 0.351562 6.5332 0.859375L6.5332 13.7207L6.63086 16.6309C6.63086 17.0898 6.94336 17.4023 7.40234 17.4023Z",
      ],
    },
    {
      // arrow.up.svg
      viewBox: "0 0 15.166 18.4473",
      paths: [
        "M0.839844 8.27148C1.07422 8.27148 1.30859 8.19336 1.45508 8.03711L3.68164 5.84961L7.39258 1.76758L11.123 5.84961L13.3496 8.03711C13.5059 8.19336 13.7305 8.27148 13.9648 8.27148C14.4531 8.27148 14.8047 7.90039 14.8047 7.42188C14.8047 7.1875 14.7266 6.98242 14.541 6.78711L8.05664 0.292969C7.87109 0.0976562 7.64648 0 7.40234 0C7.1582 0 6.93359 0.0976562 6.74805 0.292969L0.273438 6.78711C0.0878906 6.98242 0 7.1875 0 7.42188C0 7.90039 0.351562 8.27148 0.839844 8.27148ZM7.40234 18.4473C7.91016 18.4473 8.27148 18.0957 8.27148 17.5879L8.27148 4.72656L8.17383 1.81641C8.17383 1.35742 7.86133 1.04492 7.40234 1.04492C6.94336 1.04492 6.63086 1.35742 6.63086 1.81641L6.5332 4.72656L6.5332 17.5879C6.5332 18.0957 6.89453 18.4473 7.40234 18.4473Z",
      ],
    },
    {
      // return.svg
      viewBox: "0 0 20.9668 18.877",
      paths: [
        "M0.976562 11.377C0.976562 11.7773 1.30859 12.0801 1.70898 12.0996L4.91211 12.2461L17.3828 12.2461C19.6582 12.2461 20.6055 11.2012 20.6055 8.99414L20.6055 3.22266C20.6055 0.947266 19.6582 0 17.3828 0L11.9141 0C11.377 0 11.0254 0.390625 11.0254 0.869141C11.0254 1.34766 11.377 1.73828 11.9141 1.73828L17.3828 1.73828C18.418 1.73828 18.8574 2.1875 18.8574 3.22266L18.8574 8.99414C18.8574 10.0586 18.418 10.5078 17.3828 10.5078L4.91211 10.5078L1.70898 10.6445C1.30859 10.6641 0.976562 10.9668 0.976562 11.377ZM0 11.377C0 11.6113 0.0976562 11.8457 0.292969 12.0312L6.03516 17.666C6.20117 17.832 6.45508 17.9297 6.66016 17.9297C7.1875 17.9297 7.5293 17.5781 7.5293 17.0703C7.5293 16.8164 7.44141 16.6309 7.29492 16.4746L4.47266 13.7305L2.06055 11.6602L2.06055 11.084L4.47266 9.02344L7.29492 6.2793C7.44141 6.12305 7.5293 5.92773 7.5293 5.67383C7.5293 5.17578 7.1875 4.81445 6.66016 4.81445C6.45508 4.81445 6.20117 4.92188 6.03516 5.08789L0.292969 10.7227C0.0976562 10.9082 0 11.1328 0 11.377Z",
      ],
    },
    {
      // text.insert.svg
      viewBox: "0 0 20.0391 16.9238",
      paths: [
        "M19.6777 16.123C19.6777 16.5625 19.3164 16.9043 18.877 16.9043L0.78125 16.9043C0.341797 16.9043 0 16.5625 0 16.123C0 15.6836 0.341797 15.332 0.78125 15.332L18.877 15.332C19.3164 15.332 19.6777 15.6836 19.6777 16.123Z",
        "M19.6777 11.0156C19.6777 11.4551 19.3164 11.7969 18.877 11.7969L0.78125 11.7969C0.341797 11.7969 0 11.4551 0 11.0156C0 10.5664 0.341797 10.2246 0.78125 10.2246L18.877 10.2246C19.3164 10.2246 19.6777 10.5664 19.6777 11.0156Z",
        "M19.6777 5.89844C19.6777 6.33789 19.3164 6.68945 18.877 6.68945L12.2949 6.68945C11.8555 6.68945 11.5137 6.33789 11.5137 5.89844C11.5137 5.45898 11.8555 5.11719 12.2949 5.11719L18.877 5.11719C19.3164 5.11719 19.6777 5.45898 19.6777 5.89844Z",
        "M19.6777 0.791016C19.6777 1.23047 19.3164 1.57227 18.877 1.57227L12.2949 1.57227C11.8555 1.57227 11.5137 1.23047 11.5137 0.791016C11.5137 0.351562 11.8555 0 12.2949 0L18.877 0C19.3164 0 19.6777 0.351562 19.6777 0.791016Z",
        "M5.23438 4.33594L2.50977 4.33594C2.22656 4.33594 2.08008 4.48242 2.08008 4.76562L2.08008 6.45508C2.08008 6.99219 1.65039 7.42188 1.12305 7.42188C0.585938 7.42188 0.15625 6.99219 0.15625 6.45508L0.15625 4.50195C0.15625 3.16406 0.957031 2.41211 2.37305 2.41211L5.23438 2.41211Z",
        "M5.23438 1.17188L5.23438 5.68359C5.23438 6.34766 5.82031 6.65039 6.36719 6.23047L9.28711 4.00391C9.66797 3.70117 9.66797 3.1543 9.28711 2.85156L6.36719 0.625C5.82031 0.205078 5.23438 0.507812 5.23438 1.17188Z",
      ],
    },
    {
      // xmark.svg
      viewBox: "0 0 15.8472 15.4956",
      paths: [
        "M13.9929 0.27223L0.252699 14.0125C-0.0793324 14.3445-0.0890981 14.9109 0.252699 15.2429C0.594496 15.575 1.1609 15.575 1.49293 15.2429L15.2332 1.5027C15.5652 1.17067 15.575 0.604261 15.2332 0.27223C14.8914-0.0695668 14.3347-0.0793324 13.9929 0.27223ZM15.2332 14.0125L1.49293 0.27223C1.1609-0.0695668 0.58473-0.0793324 0.252699 0.27223C-0.0793324 0.614027-0.0793324 1.17067 0.252699 1.5027L13.9929 15.2429C14.325 15.575 14.9011 15.5847 15.2332 15.2429C15.5652 14.9011 15.5652 14.3445 15.2332 14.0125Z",
      ],
    },
  ];

  // Icons for a pending Claude Code permission-request question (see
  // gui/src-tauri/src/hook_bridge.rs) — swapped in for DEFAULT_ITEMS/ICONS
  // entirely while `pendingQuestion` is set (below), keyed by the icon-key
  // strings `hook_bridge::icons_for` sends.
  const XMARK_ICON = DEFAULT_ICONS[5];
  const CHECK_ICON = {
    // checkmark.svg
    viewBox: "0 0 17.1875 17.2363",
    paths: [
      "M6.36719 17.2363C6.78711 17.2363 7.11914 17.0508 7.35352 16.6895L16.582 2.1582C16.7578 1.875 16.8262 1.66016 16.8262 1.43555C16.8262 0.898438 16.4746 0.546875 15.9375 0.546875C15.5469 0.546875 15.332 0.673828 15.0977 1.04492L6.32812 15.0195L1.77734 9.0625C1.5332 8.7207 1.28906 8.58398 0.9375 8.58398C0.380859 8.58398 0 8.96484 0 9.50195C0 9.72656 0.0976562 9.98047 0.283203 10.2148L5.35156 16.6699C5.64453 17.0508 5.94727 17.2363 6.36719 17.2363Z",
    ],
  };
  const DONT_ASK_ICON = {
    // hand.raised.svg — reads as "stop asking", the closest available SF
    // Symbol to a bell-with-slash (not present in the local icon set).
    viewBox: "0 0 17.1094 22.2363",
    paths: [
      "M8.06641 21.2598C11.4941 21.2598 13.916 19.4043 15.2148 15.7422L16.9336 10.9082C17.0508 10.5762 17.1094 10.2539 17.1094 9.96094C17.1094 8.92578 16.3281 8.20312 15.332 8.20312C14.6582 8.20312 14.0527 8.60352 13.7207 9.375L13.0859 10.9375C13.0664 10.9766 13.0371 11.0059 12.998 11.0059C12.9492 11.0059 12.9297 10.9668 12.9297 10.918L12.9297 3.08594C12.9297 1.94336 12.2168 1.2207 11.123 1.2207C10.7227 1.2207 10.3613 1.36719 10.0879 1.62109C9.96094 0.615234 9.31641 0 8.34961 0C7.40234 0 6.73828 0.634766 6.5918 1.60156C6.34766 1.35742 5.99609 1.2207 5.64453 1.2207C4.62891 1.2207 3.95508 1.93359 3.95508 3.01758L3.95508 4.10156C3.69141 3.82812 3.29102 3.68164 2.88086 3.68164C1.86523 3.68164 1.16211 4.42383 1.16211 5.52734L1.16211 13.916C1.16211 18.4863 3.93555 21.2598 8.06641 21.2598ZM8.01758 19.9121C4.56055 19.9121 2.45117 17.6855 2.45117 13.7598L2.45117 5.66406C2.45117 5.24414 2.71484 4.96094 3.125 4.96094C3.52539 4.96094 3.81836 5.24414 3.81836 5.66406L3.81836 10.6543C3.81836 11.0156 4.11133 11.2598 4.42383 11.2598C4.75586 11.2598 5.05859 11.0156 5.05859 10.6543L5.05859 3.19336C5.05859 2.77344 5.32227 2.48047 5.72266 2.48047C6.13281 2.48047 6.41602 2.77344 6.41602 3.19336L6.41602 10.1562C6.41602 10.5176 6.70898 10.7617 7.03125 10.7617C7.36328 10.7617 7.65625 10.5176 7.65625 10.1562L7.65625 1.98242C7.65625 1.5625 7.93945 1.25977 8.34961 1.25977C8.74023 1.25977 9.02344 1.5625 9.02344 1.98242L9.02344 10.1562C9.02344 10.498 9.29688 10.7617 9.63867 10.7617C9.9707 10.7617 10.2637 10.498 10.2637 10.1562L10.2637 3.19336C10.2637 2.77344 10.5469 2.48047 10.9473 2.48047C11.3477 2.48047 11.6309 2.77344 11.6309 3.19336L11.6309 12.8027C11.6309 13.291 11.9141 13.5742 12.3242 13.5742C12.6758 13.5742 12.9688 13.418 13.1934 12.9297L14.5996 9.79492C14.7852 9.375 15.1465 9.22852 15.4883 9.35547C15.8691 9.49219 15.9863 9.85352 15.8105 10.332L14.0234 15.3223C12.832 18.6523 10.7422 19.9121 8.01758 19.9121Z",
    ],
  };
  const NUMBER_ICONS = [
    {
      // 1.circle.svg
      viewBox: "0 0 20.2832 19.9316",
      paths: [
        "M9.96094 19.9219C15.459 19.9219 19.9219 15.459 19.9219 9.96094C19.9219 4.46289 15.459 0 9.96094 0C4.46289 0 0 4.46289 0 9.96094C0 15.459 4.46289 19.9219 9.96094 19.9219ZM9.96094 18.2617C5.37109 18.2617 1.66016 14.5508 1.66016 9.96094C1.66016 5.37109 5.37109 1.66016 9.96094 1.66016C14.5508 1.66016 18.2617 5.37109 18.2617 9.96094C18.2617 14.5508 14.5508 18.2617 9.96094 18.2617Z",
        "M10.459 14.6094C10.957 14.6094 11.2402 14.2773 11.2402 13.7305L11.2402 6.19141C11.2402 5.625 10.9375 5.3125 10.4199 5.3125C10.0586 5.3125 9.80469 5.41016 9.38477 5.69336L7.62695 6.88477C7.41211 7.03125 7.32422 7.1875 7.32422 7.43164C7.32422 7.73438 7.56836 8.01758 7.88086 8.01758C8.02734 8.01758 8.13477 7.98828 8.35938 7.8418L9.61914 7.01172L9.6875 7.01172L9.6875 13.7305C9.6875 14.2773 9.9707 14.6094 10.459 14.6094Z",
      ],
    },
    {
      // 2.circle.svg
      viewBox: "0 0 20.2832 19.9316",
      paths: [
        "M9.96094 19.9219C15.459 19.9219 19.9219 15.459 19.9219 9.96094C19.9219 4.46289 15.459 0 9.96094 0C4.46289 0 0 4.46289 0 9.96094C0 15.459 4.46289 19.9219 9.96094 19.9219ZM9.96094 18.2617C5.37109 18.2617 1.66016 14.5508 1.66016 9.96094C1.66016 5.37109 5.37109 1.66016 9.96094 1.66016C14.5508 1.66016 18.2617 5.37109 18.2617 9.96094C18.2617 14.5508 14.5508 18.2617 9.96094 18.2617Z",
        "M7.73438 14.3848L12.7539 14.3848C13.1348 14.3848 13.4082 14.1406 13.4082 13.75C13.4082 13.3398 13.1348 13.1055 12.7539 13.1055L9.18945 13.1055L9.18945 13.0371L11.5137 10.5664C12.373 9.63867 12.9785 8.86719 12.9785 7.7832C12.9785 6.29883 11.7871 5.3125 9.98047 5.3125C8.59375 5.3125 7.37305 6.15234 7.05078 7.24609C7.01172 7.39258 7.00195 7.50977 7.00195 7.63672C7.00195 8.01758 7.25586 8.26172 7.63672 8.26172C7.97852 8.26172 8.1543 8.04688 8.33008 7.77344C8.58398 7.17773 9.10156 6.55273 10.0488 6.55273C10.9766 6.55273 11.5918 7.08984 11.5918 7.90039C11.5918 8.61328 10.9961 9.20898 10.4199 9.81445L7.29492 13.1641C7.12891 13.3496 7.04102 13.5449 7.04102 13.75C7.04102 14.1406 7.30469 14.3848 7.73438 14.3848Z",
      ],
    },
    {
      // 3.circle.svg
      viewBox: "0 0 20.2832 19.9316",
      paths: [
        "M9.96094 19.9219C15.459 19.9219 19.9219 15.459 19.9219 9.96094C19.9219 4.46289 15.459 0 9.96094 0C4.46289 0 0 4.46289 0 9.96094C0 15.459 4.46289 19.9219 9.96094 19.9219ZM9.96094 18.2617C5.37109 18.2617 1.66016 14.5508 1.66016 9.96094C1.66016 5.37109 5.37109 1.66016 9.96094 1.66016C14.5508 1.66016 18.2617 5.37109 18.2617 9.96094C18.2617 14.5508 14.5508 18.2617 9.96094 18.2617Z",
        "M9.98047 14.6094C12.0801 14.6094 13.3789 13.5059 13.3789 11.9727C13.3789 10.8203 12.5098 9.90234 11.3281 9.79492L11.3281 9.7168C12.3047 9.57031 13.0664 8.67188 13.0664 7.64648C13.0664 6.28906 11.8164 5.3125 10.0684 5.3125C8.47656 5.3125 7.27539 6.05469 7.01172 7.20703C6.98242 7.34375 6.97266 7.45117 6.97266 7.59766C6.97266 7.97852 7.24609 8.24219 7.63672 8.24219C7.98828 8.24219 8.19336 8.07617 8.30078 7.73438C8.52539 7.00195 9.13086 6.54297 10.0391 6.54297C11.0156 6.54297 11.6211 7.03125 11.6211 7.82227C11.6211 8.62305 10.9375 9.21875 10.0098 9.21875L9.39453 9.21875C9.0332 9.21875 8.7793 9.46289 8.7793 9.82422C8.7793 10.166 9.0332 10.4199 9.39453 10.4199L10.0586 10.4199C11.1621 10.4199 11.9141 11.0254 11.9141 11.9141C11.9141 12.793 11.1523 13.3691 10.0098 13.3691C8.95508 13.3691 8.35938 12.8516 8.125 12.1777C7.98828 11.8359 7.79297 11.6797 7.45117 11.6797C7.06055 11.6797 6.77734 11.9531 6.77734 12.3438C6.77734 12.4902 6.79688 12.5781 6.82617 12.7148C7.11914 13.8379 8.38867 14.6094 9.98047 14.6094Z",
      ],
    },
    {
      // 4.circle.svg
      viewBox: "0 0 20.2832 19.9316",
      paths: [
        "M9.96094 19.9219C15.459 19.9219 19.9219 15.459 19.9219 9.96094C19.9219 4.46289 15.459 0 9.96094 0C4.46289 0 0 4.46289 0 9.96094C0 15.459 4.46289 19.9219 9.96094 19.9219ZM9.96094 18.2617C5.37109 18.2617 1.66016 14.5508 1.66016 9.96094C1.66016 5.37109 5.37109 1.66016 9.96094 1.66016C14.5508 1.66016 18.2617 5.37109 18.2617 9.96094C18.2617 14.5508 14.5508 18.2617 9.96094 18.2617Z",
        "M11.2305 14.6094C11.6895 14.6094 11.9531 14.3066 11.9531 13.7891L11.9531 12.5879L12.7051 12.5879C13.0859 12.5879 13.3496 12.334 13.3496 11.9531C13.3496 11.5723 13.0957 11.3086 12.7051 11.3086L11.9531 11.3086L11.9531 6.33789C11.9531 5.72266 11.4844 5.3125 10.791 5.3125C10.1074 5.3125 9.67773 5.55664 9.28711 6.14258C8.28125 7.66602 7.07031 9.46289 6.25977 10.7812C6.06445 11.1035 5.99609 11.3379 5.99609 11.6309C5.99609 12.207 6.38672 12.5879 7.01172 12.5879L10.5176 12.5879L10.5176 13.7891C10.5176 14.2969 10.7812 14.6094 11.2305 14.6094ZM10.5176 11.3086L7.43164 11.3086L7.43164 11.25C8.17383 9.9707 9.42383 8.11523 10.4492 6.57227L10.5176 6.57227Z",
      ],
    },
  ];
  // Keyed by the icon-key strings gui/src-tauri/src/pie_menu.rs's
  // `show_pending_permission_question`/`show_pending_ask_user_question`
  // send (never raw label text — the real text goes through
  // `pendingQuestion.title`/`.labels` instead, rendered in the text panel
  // above the arc, see the `question-panel` markup below).
  const QUESTION_ICON_MAP = {
    yes: CHECK_ICON,
    no: XMARK_ICON,
    dont_ask: DONT_ASK_ICON,
    n1: NUMBER_ICONS[0],
    n2: NUMBER_ICONS[1],
    n3: NUMBER_ICONS[2],
    n4: NUMBER_ICONS[3],
    // AskUserQuestion's trailing "answer in terminal instead" escape-hatch
    // slot — see gui/src-tauri/src/pie_menu.rs's
    // `show_pending_ask_user_question`. Reuses the return-arrow glyph
    // (same as DEFAULT_ITEMS' plain Enter slot) to read as "hand off
    // elsewhere" rather than a specific answer.
    terminal: DEFAULT_ICONS[3],
  };

  // Non-null while showing a Claude Code permission-request question relayed
  // from gui/src-tauri/src/hook_bridge.rs (via the `pie-menu:open` event's
  // `question` field, set in onMount below) instead of the normal 6 fixed
  // slots — `{ icons: string[] }`, 2-4 icon-key strings long. ITEMS/ICONS
  // below derive entirely from this, so every downstream use of
  // ITEMS.length (layout, wraparound, close-index) transparently adapts to
  // however many slots the current mode actually has.
  let pendingQuestion = $state(null);
  const ITEMS = $derived(
    pendingQuestion ? pendingQuestion.icons.map((_, i) => `q${i}`) : DEFAULT_ITEMS,
  );
  const ICONS = $derived(
    pendingQuestion ? pendingQuestion.icons.map((key) => QUESTION_ICON_MAP[key] ?? XMARK_ICON) : DEFAULT_ICONS,
  );

  // Fallback geometry for the very first frame, before the backend's
  // `pie-menu:open` payload (actual monitor-derived width/height, see
  // `gui/src-tauri/src/pie_menu.rs`) arrives. Overwritten immediately after.
  const DEFAULT_ARC_WIDTH = 240;

  let arcWidth = $state(DEFAULT_ARC_WIDTH);
  // Extra logical height reserved above the arc for a question's text
  // panel — 0 outside question mode, set from the backend's `panel_height`
  // (see gui/src-tauri/src/pie_menu.rs's `question_panel_height`) in the
  // `pie-menu:open` listener below.
  let panelHeight = $state(0);
  // The actual OS window width — equals `arcWidth` outside question mode,
  // wider than it while a question card is showing (see the backend's
  // `QUESTION_PANEL_WIDTH_FRACTION`), set from `panel_width` in the
  // `pie-menu:open` listener below. `.arc-wrap`/`.question-panel` size to
  // this instead of `arcWidth` directly so the card gets the extra room;
  // the arc SVG itself keeps rendering at `arcWidth` and is re-centered
  // within the now-wider box via `svgOffsetX` below, same for each item
  // button's own position.
  let panelWidth = $state(DEFAULT_ARC_WIDTH);
  const svgOffsetX = $derived((panelWidth - arcWidth) / 2);
  // The bounding box keeps the same 2:1 aspect ratio the Rust side sizes the
  // window to, even though the visible arc (130°, see ARC_SPAN below) no
  // longer reaches all the way down to the box's bottom corners the way a
  // full 180° semicircle would — that empty space below the arc's open ends
  // is intentional, it's what makes the arc read as floating.
  const arcHeight = $derived(arcWidth / 2);
  const R_OUTER = $derived(arcWidth / 2);
  // The arc's pivot point — horizontal and vertical used to coincide
  // (`PIVOT_Y === R_OUTER`) purely because `arcHeight` happened to equal
  // `arcWidth / 2`. Once a question panel can make the box taller than that
  // without changing the arc's own width, they need to be tracked
  // separately: PIVOT_X stays the horizontal center, PIVOT_Y is the
  // *bottom* of the box (arc + panel), so the arc keeps floating at the
  // bottom edge exactly like before panels existed (PIVOT_Y === R_OUTER
  // whenever panelHeight is 0).
  const PIVOT_X = $derived(R_OUTER);
  const PIVOT_Y = $derived(arcHeight + panelHeight);
  // A thick glass "pill" band rather than a thin ring — thick enough that
  // the selection highlight arc (same stroke width) reads as nested inside
  // it rather than a separate shape overlapping it.
  const BAND_FRACTION = 0.44;
  const BAND_THICKNESS = $derived(R_OUTER * BAND_FRACTION);
  const R_INNER = $derived(R_OUTER - BAND_THICKNESS);
  // Both the band and every item sit on this centerline radius.
  const R_MID = $derived((R_OUTER + R_INNER) / 2);
  const ITEM_SIZE = $derived(R_OUTER * 0.3);

  // Only a partial arc (~130°), not a full 180° semicircle — measured from
  // the positive x-axis (0 = right, 90 = straight up, 180 = left).
  const ARC_SPAN = 130;
  const ARC_START = 90 + ARC_SPAN / 2; // 155
  const ARC_END = 90 - ARC_SPAN / 2; // 25
  // Resting angle every item (and the highlight) collapses back to while
  // closed — dead center of the arc, so opening reads as the fan unfolding
  // outward from one point.
  const ANGLE_REST = 90;
  // Angular width of the nested selection-highlight arc segment: kept just
  // barely wider than its own round end-caps, so it reads as close to a
  // plain circle (just a little wider) rather than an elongated segment.
  const HIGHLIGHT_SPAN = $derived(((ARC_START - ARC_END) / (ITEMS.length - 1)) * 0.16);
  // Radial thickness of the highlight block itself, and how far it sits
  // inset from the band's own inner/outer edges — like a small block
  // sliding inside a tube, with just a sliver of clearance on both sides
  // rather than a large gap.
  const HIGHLIGHT_THICKNESS = $derived(BAND_THICKNESS * 0.82);
  // Items are placed on an inset range, not the band's full [ARC_END,
  // ARC_START] extent — that inset exactly equals the highlight's own half
  // width, so a highlight centered on the first/last item never needs to
  // extend past the band's own bounds. (An earlier version instead clamped
  // the highlight's rendered edges directly, which kept it from poking out
  // but shifted its visual center off of the actual selected item at the
  // two ends — this fixes that at the root instead.)
  const ITEM_ARC_START = $derived(ARC_START - HIGHLIGHT_SPAN / 2);
  const ITEM_ARC_END = $derived(ARC_END + HIGHLIGHT_SPAN / 2);
  // How much of the band's own arc length this stroke actually is, in SVG
  // user units — used to "draw" it in with stroke-dasharray/dashoffset
  // rather than fading the whole shape in uniformly (see bandArcLength).
  const BAND_ARC_LENGTH = $derived(R_MID * ((ARC_START - ARC_END) * Math.PI) / 180);
  // Item reveal is staggered left-to-right (see ITEMS.map reveal below)
  // rather than every item fading in from the center simultaneously.
  const REVEAL_STAGGER = 0.12;

  function angleFor(i, n) {
    if (n <= 1) return ANGLE_REST;
    return ITEM_ARC_START + ((ITEM_ARC_END - ITEM_ARC_START) * i) / (n - 1);
  }

  function polar(cx, cy, r, angleDeg) {
    const rad = (angleDeg * Math.PI) / 180;
    return [cx + r * Math.cos(rad), cy - r * Math.sin(rad)];
  }

  // SVG path for an open arc stroke from startAngle to endAngle at radius r,
  // centered on the pivot (cx, cy) — the box's bottom-center, same point the
  // half-circle geometry has always been centered on. `stroke-linecap:round`
  // (set in the markup below) turns each open end into a rounded cap.
  function describeArc(cx, cy, r, startAngle, endAngle) {
    const [sx, sy] = polar(cx, cy, r, startAngle);
    const [ex, ey] = polar(cx, cy, r, endAngle);
    const largeArc = Math.abs(startAngle - endAngle) > 180 ? 1 : 0;
    return `M ${sx} ${sy} A ${r} ${r} 0 ${largeArc} 1 ${ex} ${ey}`;
  }

  // --- Animation -----------------------------------------------------------
  //
  // Reuses the actual animation approach from the referenced oled-ui-astra
  // project (`Core/Src/astra/ui/item/item.h`, `Animation::move()`): a
  // first-order proportional-step ease toward the target each tick, not a
  // physically-simulated spring. Their original is tick-rate-locked
  // (`pos += (target - pos) / (100 - speed)` once per firmware loop
  // iteration); `stepEase` below adapts the same fraction-per-tick to be
  // frame-rate independent by treating it as continuous exponential decay
  // referenced to a 60Hz tick, since a browser rAF loop's dt varies.
  function stepEase(pos, target, speed, dt) {
    if (Math.abs(target - pos) < 0.01) return target;
    const fractionPerTick = 1 / (100 - speed);
    const decay = Math.pow(1 - fractionPerTick, dt * 60);
    return target + (pos - target) * decay;
  }

  const OPEN_SPEED = 90;
  const HIGHLIGHT_SPEED = 94;
  // How the whole assembled menu (band + highlight + items, as one rigid
  // group — see CLOSE_SLIDE_DISTANCE/groupOpacity/groupTranslateY below)
  // slides away on close: straight down and out, fast, uniformly, instead
  // of reversing the same left-to-right draw-in/stagger used for opening.
  const CLOSE_SLIDE_SPEED = 92;
  const CLOSE_SLIDE_DISTANCE = 26;
  const OPEN_EPS = 0.002;
  const ANGLE_EPS = 0.05;

  let openPos = $state(0);
  let openTarget = 0;
  let closeProgress = $state(0);
  let closeTarget = 0;

  // The selection highlight's own angle — items themselves no longer move
  // (they sit permanently at their resting slot, see angleFor + the reveal
  // stagger below), so this is the only angle that still eases, sliding
  // between slots as the highlighted selection changes.
  let highlightAngle = $state(ANGLE_REST);
  let highlightTarget = ANGLE_REST;

  let selected = $state(0);
  /** @type {"closed" | "open" | "closing"} */
  let phase = $state("closed");
  let openedAt = 0;
  let unlisten;

  let rafId = null;
  let lastT = 0;
  let pendingCloseCallback = null;
  let closeFired = false;
  let unlistenMove;

  function tick(now) {
    const dt = Math.min((now - lastT) / 1000, 1 / 30);
    lastT = now;

    let moving = false;

    // Frozen while closing — the open/reveal progress no longer reverses;
    // the whole assembled menu instead slides away as one rigid unit via
    // closeProgress below, so the band + items stay at their fully-open
    // appearance throughout.
    if (phase !== "closing") {
      openPos = stepEase(openPos, openTarget, OPEN_SPEED, dt);
      if (Math.abs(openTarget - openPos) > OPEN_EPS) moving = true;
    }

    highlightAngle = stepEase(highlightAngle, highlightTarget, HIGHLIGHT_SPEED, dt);
    if (Math.abs(highlightTarget - highlightAngle) > ANGLE_EPS) moving = true;

    if (phase === "closing") {
      closeProgress = stepEase(closeProgress, closeTarget, CLOSE_SLIDE_SPEED, dt);
      if (Math.abs(closeTarget - closeProgress) > OPEN_EPS) moving = true;
    }

    if (moving) {
      rafId = requestAnimationFrame(tick);
    } else {
      rafId = null;
      lastT = 0;
      if (phase === "closing" && !closeFired) {
        closeFired = true;
        finishClose();
      }
    }
  }

  // Resets everything for the next open, once this close has actually
  // finished (the overlay window itself is about to be hidden by
  // pendingCloseCallback, so this reset is invisible).
  function finishClose() {
    phase = "closed";
    openPos = 0;
    openTarget = 0;
    closeProgress = 0;
    closeTarget = 0;
    highlightAngle = ANGLE_REST;
    highlightTarget = ANGLE_REST;
    pendingCloseCallback?.();
    pendingCloseCallback = null;
    // Reset after the callback fires (which needs to know it was a
    // question being answered) — the next "pie-menu:open" event sets this
    // again regardless, so this is just hygiene, not load-bearing.
    pendingQuestion = null;
  }

  function ensureLoop() {
    if (rafId == null) {
      lastT = 0;
      rafId = requestAnimationFrame(tick);
    }
  }

  function move(delta) {
    // Wraps around at both ends — moving right past the last slot lands on
    // the first one and vice versa, rather than clamping. This matters for
    // mic-tap navigation specifically: the tap classifier routinely
    // misreads a double tap as a single one, so double-tap-moves-left was
    // dropped as unreliable in favor of single-tap-with-wraparound, which
    // reaches every slot using only the more reliable single-tap gesture
    // (see gui/src-tauri/src/mic_tap.rs's finalize_group).
    const next = (selected + delta + ITEMS.length) % ITEMS.length;
    if (next === selected) return;
    selected = next;
    highlightTarget = angleFor(selected, ITEMS.length);
    ensureLoop();
  }

  function closeWithAnim(after) {
    if (phase !== "open") return;
    phase = "closing";
    // The highlight is left exactly where it is — no repositioning — and
    // slides away together with the rest of the menu as one single rigid
    // unit (see groupOpacity/groupTranslateY), not as a separate step.
    pendingCloseCallback = after;
    closeFired = false;
    closeTarget = 1;
    ensureLoop();
    // Safety net: the ease should always cross the epsilon threshold
    // quickly, but guarantee the callback still fires even if a frame stall
    // or tuning change ever left it short of the target indefinitely.
    setTimeout(() => {
      if (!closeFired) {
        closeFired = true;
        finishClose();
      }
    }, 400);
  }

  // Only the last slot ("close"/xmark, matching CLOSE_INDEX in
  // gui/src-tauri/src/pie_menu.rs) actually closes the menu — every other
  // slot fires its action and leaves the menu open, so pressing e.g. "down"
  // several times in a row doesn't require reopening it each time.
  //
  // A pending question (see pendingQuestion above) is a different case:
  // there's no dedicated close slot, because there's nothing to leave open
  // for — a question is answered once (resolving permission_server's held
  // http connection directly, whether it's a Permission or an
  // AskUserQuestion — see that module's doc comment), so every choice
  // closes the overlay, matching the close-slot's own animation/callback
  // shape.
  function confirmSelection() {
    const index = selected;
    if (pendingQuestion) {
      closeWithAnim(() => pieMenuAnswerQuestion(index));
    } else if (index === ITEMS.length - 1) {
      closeWithAnim(() => pieMenuSelect(index));
    } else {
      // pie_menu_select (Rust) deliberately hands OS focus back to whatever
      // window was active before the menu opened for a moment, so the
      // simulated keystroke lands there instead of on this still-open
      // overlay — but that handoff itself fires a blur event here, which
      // onBlur normally treats as "clicked away, cancel the menu". Suppress
      // that for long enough to cover the round trip (60ms handoff +
      // action + 30ms reclaim, see pie_menu_select) plus margin.
      suppressBlurUntil = Date.now() + 400;
      pieMenuSelect(index);
    }
  }

  function cancel() {
    closeWithAnim(() => pieMenuClose());
  }

  function onKeydown(e) {
    if (phase !== "open") return;
    switch (e.key) {
      case "ArrowLeft":
      case "ArrowUp":
        e.preventDefault();
        move(-1);
        break;
      case "ArrowRight":
      case "ArrowDown":
        e.preventDefault();
        move(1);
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        confirmSelection();
        break;
      case "Escape":
        e.preventDefault();
        cancel();
        break;
    }
  }

  // The overlay window is real-focused on open, but the very first focus
  // event right after `show()` can race a spurious blur — ignore blur for a
  // short grace period after opening. Also ignored while suppressBlurUntil
  // is in the future — see confirmSelection's non-close branch.
  //
  // A pending Claude Code question (see pendingQuestion) never auto-cancels
  // on blur at all, unlike the normal 6-slot menu: anything else briefly
  // stealing OS focus (another window popping up, the user alt-tabbing to
  // read more terminal output, ...) is completely routine while a person
  // takes a moment to actually read/decide on a question, and closing the
  // overlay here doesn't cancel the *real* question still waiting in
  // Claude Code's own terminal — it would just silently hide the only UI
  // that shows what it is and lets the pairing button answer it, with no
  // feedback that that happened. The backend's own pending-answer state
  // (`PENDING_ANSWER` in gui/src-tauri/src/pie_menu.rs) has no lifetime tied
  // to this window's focus either, so there's nothing to invalidate by
  // staying open — only Escape (onKeydown) or an actual answer closes it.
  let suppressBlurUntil = 0;
  function onBlur() {
    if (Date.now() < suppressBlurUntil) return;
    if (pendingQuestion) return;
    if (phase === "open" && Date.now() - openedAt > 250) cancel();
  }

  onMount(() => {
    window.addEventListener("keydown", onKeydown);
    window.addEventListener("blur", onBlur);
    listen("pie-menu:open", (event) => {
      if (event.payload?.width) arcWidth = event.payload.width;
      panelWidth = event.payload?.panel_width ?? arcWidth;
      panelHeight = event.payload?.panel_height ?? 0;
      // Set before reading ITEMS.length below — ITEMS/ICONS derive from
      // this, so the very first frame already reflects however many slots
      // this open actually has (6 fixed, or however many icons a pending
      // question sent).
      pendingQuestion = event.payload?.question ?? null;
      selected = 0;
      openedAt = Date.now();
      phase = "open";
      openTarget = 1;
      // Placed directly at the first slot, not eased there — it should
      // already be sitting on the first item from the very first rendered
      // frame of the reveal, not visibly slide in from center.
      highlightAngle = angleFor(selected, ITEMS.length);
      highlightTarget = angleFor(selected, ITEMS.length);
      ensureLoop();
    }).then((fn) => {
      unlisten = fn;
    });
    // Mirrors the keyboard's Right handling in onKeydown — emitted by
    // gui/src-tauri/src/pie_menu.rs's `navigate` on a single mic tap while
    // this menu is already open. (The pairing button's confirm doesn't go
    // through an event like this — it just simulates a real Enter keypress,
    // which lands on this already-focused window and hits onKeydown's own
    // Enter case directly.)
    listen("pie-menu:move", (event) => {
      if (phase === "open") move(event.payload);
    }).then((fn) => {
      unlistenMove = fn;
    });
  });

  onDestroy(() => {
    window.removeEventListener("keydown", onKeydown);
    window.removeEventListener("blur", onBlur);
    unlisten?.();
    unlistenMove?.();
    if (rafId != null) cancelAnimationFrame(rafId);
  });

  const fade = $derived(Math.max(0, Math.min(1, openPos)));
  // Whole-group close transform: applied once, on the outer wrapper, to the
  // fully-open band + highlight + items together as one rigid unit — not a
  // reverse of the individual left-to-right reveal used for opening.
  const groupOpacity = $derived(phase === "closing" ? 1 - closeProgress : fade);
  const groupTranslateY = $derived(phase === "closing" ? closeProgress * CLOSE_SLIDE_DISTANCE : 0);
  const bandArcD = $derived(describeArc(PIVOT_X, PIVOT_Y, R_MID, ARC_START, ARC_END));
  // The band's path starts (its `M` point) at ARC_START — the left side —
  // and draws toward ARC_END on the right, so shrinking the dashoffset from
  // the full arc length down to 0 reveals it left-to-right, like a stroke
  // being drawn, instead of fading the whole shape in uniformly.
  const bandDashOffset = $derived(BAND_ARC_LENGTH * (1 - fade));
  // Item i's own reveal progress: a window of the overall open progress
  // offset by its index, so items pop in one after another left-to-right
  // instead of all fading in from the center at once. Each item's window
  // still reaches 1 by the time `fade` reaches 1, and the whole reveal
  // reverses symmetrically (right-to-left) on close.
  function itemProgress(i, n) {
    if (n <= 1) return fade;
    const denom = 1 - (n - 1) * REVEAL_STAGGER;
    return Math.max(0, Math.min(1, (fade - i * REVEAL_STAGGER) / denom));
  }
  // Always symmetric around highlightAngle — items sit on the inset
  // [ITEM_ARC_END, ITEM_ARC_START] range specifically so this never needs
  // clamping to stay within the band's own bounds (see ITEM_ARC_START).
  const highlightArcD = $derived(
    describeArc(PIVOT_X, PIVOT_Y, R_MID, highlightAngle + HIGHLIGHT_SPAN / 2, highlightAngle - HIGHLIGHT_SPAN / 2),
  );
</script>

<div class="stage">
  <div
    class="arc-wrap"
    style="width:{panelWidth}px; height:{arcHeight + panelHeight}px; opacity:{groupOpacity}; transform: translateY({groupTranslateY}px);"
  >
    <svg
      class="arc-svg"
      style="left:{svgOffsetX}px; transform: translateY({(1 - fade) * 12}px) scale({0.9 + fade * 0.1});"
      viewBox="0 0 {arcWidth} {arcHeight + panelHeight}"
      width={arcWidth}
      height={arcHeight + panelHeight}
    >
      <!-- Solid pure-white band — "liquid glass" here means the motion
           (the left-to-right draw-in, the sliding highlight), not the
           material/color, so no frosted tint or sheen gradient. Drawn in
           left-to-right via dasharray/dashoffset rather than fading in
           uniformly — see bandDashOffset. -->
      <path
        d={bandArcD}
        fill="none"
        stroke="#fff"
        stroke-width={BAND_THICKNESS}
        stroke-linecap="round"
        stroke-dasharray={BAND_ARC_LENGTH}
        stroke-dashoffset={bandDashOffset}
      />

      {#if phase !== "closed"}
        <!-- Selection highlight: a small arc-shaped block nested at the same
             centerline radius as the band but thinner than the band's own
             thickness, so it reads as a block sliding inside a tube with
             visible clearance on both sides. -->
        <path
          d={highlightArcD}
          fill="none"
          stroke="var(--accent)"
          stroke-width={HIGHLIGHT_THICKNESS}
          stroke-linecap="round"
          opacity={0.85 * fade}
        />
      {/if}
    </svg>

    {#if pendingQuestion}
      {@const isPermission = pendingQuestion.kind === "permission"}
      {@const detail = pendingQuestion.detail ?? ""}
      <!-- Real question/permission text, in the space question_panel_height
           (gui/src-tauri/src/pie_menu.rs) reserves above the arc, rendered as
           a polished "Claude Code is asking you…" card: a quiet brand header,
           a prominent title (the question, or a synthesized "Allow <tool>?"
           for a permission), an optional monospaced detail block (permission
           mode only — the concrete command/target), then one row per option.
           The arc's own tiny icon slots below stay the tactile selector and
           mic-tap target; this card is its readable mirror, with the row whose
           index === `selected` (while open) highlighted in lockstep with the
           arc highlight below. Each row reuses the same per-slot icon and the
           same select-then-confirm path an arc item uses, so a mouse click is
           an optional extra on top of the arc/keys/mic-tap. Plain HTML (not
           SVG <text>) for free text wrapping; a sibling of the SVG so it can
           use normal CSS layout. No independent opacity/close-animation of its
           own — it inherits `groupOpacity` from `.arc-wrap`, fading/sliding
           away with the rest of the menu. The `kind`/`detail` fields are read
           additively: an older backend that omits them degrades to a plain
           question card (kind → "question", no detail block). -->
      <div class="question-panel" style="height:{panelHeight}px;">
        <div class="qc-card">
          <div class="qc-header">
            <span class="qc-star" aria-hidden="true">✳</span>
            <span class="qc-brand">Claude Code</span>
            {#if isPermission}
              <span class="qc-dot" aria-hidden="true">·</span>
              <span class="qc-kind">Permission</span>
            {/if}
          </div>

          <div class="qc-title">
            {#if isPermission}Allow {pendingQuestion.title}?{:else}{pendingQuestion.title}{/if}
          </div>

          {#if isPermission && detail}
            <div class="qc-detail">{detail}</div>
          {/if}

          <div class="qc-options">
            {#each pendingQuestion.labels as label, i (i)}
              {@const icon = ICONS[i]}
              <button
                type="button"
                class="qc-row"
                class:selected={phase === "open" && i === selected}
                onclick={() => {
                  selected = i;
                  highlightTarget = angleFor(selected, ITEMS.length);
                  confirmSelection();
                }}
              >
                <svg
                  class="qc-row-icon"
                  viewBox={icon.viewBox}
                  width="18"
                  height="18"
                  aria-hidden="true"
                >
                  {#each icon.paths as d (d)}
                    <path {d} fill="currentColor" />
                  {/each}
                </svg>
                <span class="qc-row-label">{label}</span>
              </button>
            {/each}
          </div>
        </div>
      </div>
    {/if}

    {#each ITEMS as label, i (label)}
      {@const rad = (angleFor(i, ITEMS.length) * Math.PI) / 180}
      {@const x = PIVOT_X + R_MID * Math.cos(rad) + svgOffsetX}
      {@const y = PIVOT_Y - R_MID * Math.sin(rad)}
      {@const progress = itemProgress(i, ITEMS.length)}
      {@const icon = ICONS[i]}
      <button
        type="button"
        class="item"
        class:selected={phase === "open" && i === selected}
        style="left:{x}px; top:{y}px; width:{ITEM_SIZE}px; height:{ITEM_SIZE}px; opacity:{progress}; transform: translate(-50%, -50%) scale({0.55 + 0.45 * progress});"
        onclick={() => {
          selected = i;
          highlightTarget = angleFor(selected, ITEMS.length);
          confirmSelection();
        }}
      >
        <svg class="item-icon" viewBox={icon.viewBox} width={ITEM_SIZE * 0.44} height={ITEM_SIZE * 0.44}>
          {#each icon.paths as d (d)}
            <path {d} fill="currentColor" />
          {/each}
        </svg>
      </button>
    {/each}
  </div>
</div>

<style>
  :global(html),
  :global(body),
  :global(#app) {
    background: transparent !important;
  }

  .stage {
    position: fixed;
    inset: 0;
    display: flex;
    align-items: flex-end;
    justify-content: center;
    pointer-events: none;
  }

  .arc-wrap {
    position: relative;
    pointer-events: none;
  }

  .arc-svg {
    position: absolute;
    top: 0;
    /* `left` is set inline (svgOffsetX) — 0 outside question mode, and the
       centering offset needed to keep the arc's own (unchanged) diameter
       centered within a wider `.arc-wrap` while a question card is showing
       (see QUESTION_PANEL_WIDTH_FRACTION in gui/src-tauri/src/pie_menu.rs). */
    pointer-events: none;
    filter: drop-shadow(0 8px 16px rgba(0, 0, 0, 0.4));
  }

  /* --- Claude Code sync card -------------------------------------------
     Replaces the old flat title + option-pills panel with one polished card
     that reads as "Claude Code is asking you something." It lives in the
     space `question_panel_height` (gui/src-tauri/src/pie_menu.rs) reserves
     above the arc, and is top-anchored inside it: the reserved height is
     budgeted generously (title + one row each, per that file's
     QUESTION_PANEL_* constants), so any slack collects below the card as a
     deliberate floating gap before the arc, exactly like the old panel did.
     The card is intentionally compact and never taller than its reserved
     space — `max-height:100%` + `overflow:hidden` guarantee it can't spill
     down onto the arc even in the rare all-worst-case-lengths question. */
  .question-panel {
    position: absolute;
    top: 0;
    left: 50%;
    transform: translateX(-50%);
    width: 94%;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    justify-content: flex-start;
    pointer-events: none;
    overflow: hidden;
  }

  /* One solid-white "liquid glass" surface (same material/shadow language as
     the arc band), left-aligned like a real dialog rather than the old
     centered pills. */
  .qc-card {
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 11px;
    max-height: 100%;
    overflow: hidden;
    padding: 15px 17px;
    background: #fff;
    border-radius: 18px;
    text-align: left;
    filter: drop-shadow(0 8px 16px rgba(0, 0, 0, 0.4));
  }

  /* Quiet brand line — small and low-contrast, with only the star tinted the
     accent color so it reads as a mark, not a heading. */
  .qc-header {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    font-weight: 600;
    line-height: 1;
    letter-spacing: 0.01em;
    color: #8a8a8e;
  }
  .qc-star {
    color: var(--accent);
    font-size: 13px;
  }
  .qc-brand {
    color: #6b6b70;
  }
  .qc-dot {
    color: #c7c7cc;
  }
  .qc-kind {
    color: #8a8a8e;
  }

  /* The one line worth the most weight: the question, or a synthesized
     "Allow <tool>?" for a permission. Up to 3 lines, then ellipsis. Sizing
     tracks gui/src-tauri/src/pie_menu.rs's QUESTION_PANEL_TITLE_HEIGHT —
     kept in sync by hand, see that file's matching comment. */
  .qc-title {
    font-size: 16px;
    line-height: 1.3;
    font-weight: 700;
    color: #1c1c1e;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  /* Permission mode only: the concrete command/target being requested, shown
     monospaced in a faintly accent-tinted rounded box so it reads as literal
     text to inspect, not prose. Long commands wrap, then clamp to 3 lines. */
  .qc-detail {
    font-family: ui-monospace, "Cascadia Code", Consolas, "Courier New", monospace;
    font-size: 13px;
    line-height: 1.4;
    color: #2b2b2e;
    background: color-mix(in srgb, var(--accent) 9%, #f2f2f4);
    border-radius: 10px;
    padding: 9px 12px;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
  }

  /* The options block is deliberately narrower than the title/detail above
     it and centered — reads as a trapezoid (wide top, narrow bottom)
     instead of a plain rectangle, and gives the title/detail the card's
     full width for actual content (the reason they needed more room in the
     first place) while option labels ("Allow", "Deny", ...) rarely need
     that much width anyway. */
  .qc-options {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 86%;
    align-self: center;
  }

  /* One row per option — the readable mirror of an arc slot. Row height
     tracks gui/src-tauri/src/pie_menu.rs's QUESTION_PANEL_ROW_HEIGHT (kept in
     sync by hand). Reset from the <button> defaults; clickable (same
     select-then-confirm path the arc items use) as a mouse nicety, while the
     arc/keys/mic-tap stay the primary selector. */
  .qc-row {
    display: flex;
    align-items: center;
    gap: 10px;
    box-sizing: border-box;
    width: 100%;
    margin: 0;
    padding: 9px 13px;
    border: none;
    border-radius: 12px;
    background: #f1f1f4;
    color: #3a3a3c;
    font: inherit;
    font-size: 14px;
    font-weight: 500;
    text-align: left;
    cursor: pointer;
    pointer-events: auto;
    transition: background 0.12s ease, color 0.12s ease, transform 0.12s ease, box-shadow 0.12s ease;
  }

  /* Mirrors the arc's own accent highlight: the row at `selected` (while
     open) fills with the accent, brightens its text/icon, and lifts slightly
     — the same controller-focus language as the arc's sliding block below. */
  .qc-row.selected {
    background: var(--accent);
    color: #fff;
    font-weight: 700;
    transform: scale(1.02);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 42%, transparent);
  }

  /* Its paths use fill:currentColor, so it inherits the row's text color and
     flips to white on the selected row exactly like the arc icons do. */
  .qc-row-icon {
    display: block;
    flex: 0 0 auto;
  }

  .qc-row-label {
    flex: 1 1 auto;
    min-width: 0;
    line-height: 1.25;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  /* No background/border chrome — just the icon itself sitting directly on
     the band, no separate circular button shape underneath each option.
     Dark by default for contrast against the now-solid-white band; flips to
     white when the accent-colored highlight block is sitting behind it. */
  .item {
    position: absolute;
    display: grid;
    place-items: center;
    background: none;
    border: none;
    color: #1c1c1e;
    cursor: pointer;
    pointer-events: auto;
    transition: color 0.12s ease;
  }

  .item.selected {
    color: #fff;
  }

  .item-icon {
    pointer-events: none;
  }
</style>
