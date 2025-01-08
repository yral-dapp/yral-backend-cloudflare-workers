-- Migration number: 0001 	 2024-12-24T15:36:55.494Z
CREATE TABLE work_items (
    post_id INT NOT NULL, -- for reference
    video_id VARCHAR(255) NOT NULL,
    publisher_user_id VARCHAR(255) NOT NULL,
    PRIMARY KEY (video_id) -- doesn't matter; we wont be indexing into this table anyways
);
