# Headless Wayland session for capturing Cuotai's Waybar example (PRD 0014).
# sway runs on wlroots' headless backend with the software renderer, Waybar
# runs the example module against a fixture, and grim takes the screenshot.
# Nothing here talks to a provider or reads a credential.
FROM alpine:3.20

RUN apk add --no-cache \
        sway swaybg waybar grim jq \
        font-noto font-noto-emoji \
        mesa-dri-gallium libcap-setcap dbus \
    && setcap -r /usr/bin/sway \
    && dbus-uuidgen --ensure=/etc/machine-id \
    && adduser -D -u 1000 shot \
    && mkdir -p /fixture /out \
    && chown shot:shot /out

COPY sway.conf /etc/widget-shots/sway.conf
COPY waybar/ /etc/widget-shots/waybar/
COPY entrypoint.sh /usr/local/bin/widget-shots
RUN chmod 0755 /usr/local/bin/widget-shots /etc/widget-shots/waybar/cuotai-waybar.sh

USER shot
ENTRYPOINT ["/usr/local/bin/widget-shots"]
