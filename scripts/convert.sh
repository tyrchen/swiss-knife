#!/bin/bash

# Check if a video file is provided
if [ $# -eq 0 ]; then
    echo "Usage: $0 <video_file>"
    exit 1
fi

VIDEO_FILE="$1"
VIDEO_NAME=$(basename "$VIDEO_FILE" | sed 's/\.[^.]*$//')
TMP_DIR="/tmp"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}🎬 Processing video: $VIDEO_FILE${NC}"

# Step 1: Extract audio from video and split if necessary
echo -e "${YELLOW}📢 Checking audio duration...${NC}"

# Get video duration first
DURATION=$(ffprobe -v error -show_entries format=duration -of default=noprint_wrappers=1:nokey=1 "$VIDEO_FILE" 2>/dev/null | cut -d'.' -f1)
echo -e "${YELLOW}Video duration: ${DURATION} seconds${NC}"

# Clear the transcript file
TRANSCRIPT_FILE="${TMP_DIR}/${VIDEO_NAME}_transcript.txt"
> "$TRANSCRIPT_FILE"

# If video is longer than 1300 seconds, extract in chunks
if [ "$DURATION" -gt 1300 ]; then
    echo -e "${YELLOW}⚠️ Video longer than 1300 seconds, extracting audio in chunks...${NC}"

    # Calculate number of chunks needed
    NUM_CHUNKS=$(( ($DURATION + 1299) / 1300 ))
    echo -e "${YELLOW}Will create ${NUM_CHUNKS} chunks${NC}"

    # Process each chunk
    for (( i=0; i<$NUM_CHUNKS; i++ )); do
        START_TIME=$((i * 1300))
        CHUNK_DURATION=1300

        # For the last chunk, calculate remaining duration
        if [ $((START_TIME + CHUNK_DURATION)) -gt $DURATION ]; then
            CHUNK_DURATION=$((DURATION - START_TIME))
        fi

        echo -e "${YELLOW}Processing chunk $((i+1))/${NUM_CHUNKS} (${START_TIME}s - $((START_TIME + CHUNK_DURATION))s)...${NC}"

        # Define file paths
        CHUNK_AUDIO_FILE="${TMP_DIR}/${VIDEO_NAME}_chunk_${i}.mp3"
        CHUNK_TRANSCRIPT_FILE="${TMP_DIR}/${VIDEO_NAME}_chunk_${i}_transcript.txt"

        # Check if chunk transcript already exists
        if [ -f "$CHUNK_TRANSCRIPT_FILE" ] && [ -s "$CHUNK_TRANSCRIPT_FILE" ]; then
            echo -e "${GREEN}Chunk $((i+1)) transcript already exists, using cached version...${NC}"
            CHUNK_TRANSCRIPT=$(cat "$CHUNK_TRANSCRIPT_FILE")
        else
            # Check if audio chunk already exists
            if [ -f "$CHUNK_AUDIO_FILE" ]; then
                echo -e "${GREEN}Chunk $((i+1)) audio already exists, using cached version...${NC}"
            else
                # Extract audio chunk directly from video
                echo -e "${YELLOW}Extracting audio chunk $((i+1))...${NC}"
                ffmpeg -i "$VIDEO_FILE" -ss $START_TIME -t $CHUNK_DURATION -vn -acodec mp3 -ab 32k -ar 16000 -ac 1 -y "$CHUNK_AUDIO_FILE" 2>/dev/null

                if [ $? -ne 0 ]; then
                    echo -e "${RED}❌ Failed to extract audio chunk $((i+1))${NC}"
                    exit 1
                fi
            fi

            # Check chunk file size and compress if needed
            CHUNK_SIZE=$(stat -f%z "$CHUNK_AUDIO_FILE" 2>/dev/null || stat -c%s "$CHUNK_AUDIO_FILE" 2>/dev/null)
            CHUNK_SIZE_MB=$((CHUNK_SIZE / 1024 / 1024))

            if [ $CHUNK_SIZE_MB -gt 24 ]; then
                echo -e "${YELLOW}Chunk too large (${CHUNK_SIZE_MB}MB), compressing...${NC}"
                COMPRESSED_CHUNK="${TMP_DIR}/${VIDEO_NAME}_chunk_${i}_compressed.mp3"
                ffmpeg -i "$CHUNK_AUDIO_FILE" -acodec mp3 -ab 24k -ar 16000 -ac 1 -y "$COMPRESSED_CHUNK" 2>/dev/null
                mv "$COMPRESSED_CHUNK" "$CHUNK_AUDIO_FILE"
            fi

            # Transcribe chunk using gpt-4o-transcribe
            echo -e "${YELLOW}Transcribing chunk $((i+1))...${NC}"
            CHUNK_RESPONSE=$(curl -s -w "\nHTTP_STATUS:%{http_code}" https://api.openai.com/v1/audio/transcriptions \
              -H "Authorization: Bearer $OPENAI_API_KEY" \
              -H "Content-Type: multipart/form-data" \
              -F file="@$CHUNK_AUDIO_FILE" \
              -F model="gpt-4o-transcribe" \
              -F response_format="json" \
              -F language="zh")

            # Extract HTTP status
            HTTP_STATUS=$(echo "$CHUNK_RESPONSE" | grep "HTTP_STATUS:" | cut -d: -f2)
            RESPONSE_BODY=$(echo "$CHUNK_RESPONSE" | sed '/HTTP_STATUS:/d')

            if [ "$HTTP_STATUS" != "200" ]; then
                echo -e "${RED}❌ API call failed for chunk $((i+1)) with status: $HTTP_STATUS${NC}"
                echo -e "${RED}Response: $RESPONSE_BODY${NC}"
                exit 1
            fi

            # Extract text from JSON response
            CHUNK_TRANSCRIPT=$(echo "$RESPONSE_BODY" | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    text = data.get('text', '')
    if text:
        print(text)
    else:
        print('Error: No text in response', file=sys.stderr)
        sys.exit(1)
except Exception as e:
    print(f'Error parsing JSON: {e}', file=sys.stderr)
    sys.exit(1)
")

            if [ -z "$CHUNK_TRANSCRIPT" ]; then
                echo -e "${RED}❌ Failed to extract transcript for chunk $((i+1))${NC}"
                exit 1
            fi

            # Save chunk transcript for caching
            echo "$CHUNK_TRANSCRIPT" > "$CHUNK_TRANSCRIPT_FILE"
            echo -e "${GREEN}✅ Chunk $((i+1)) transcribed and cached${NC}"
        fi

        # Append to full transcript with a separator
        if [ $i -gt 0 ]; then
            echo " " >> "$TRANSCRIPT_FILE"
        fi
        echo "$CHUNK_TRANSCRIPT" >> "$TRANSCRIPT_FILE"
    done

    # Read the complete transcript
    TRANSCRIPT=$(cat "$TRANSCRIPT_FILE")
    echo -e "${GREEN}✅ All chunks merged into complete transcript${NC}"

else
    # Process as single file if duration <= 1300 seconds
    AUDIO_FILE="${TMP_DIR}/${VIDEO_NAME}.mp3"

    # Check if transcript already exists
    if [ -f "$TRANSCRIPT_FILE" ] && [ -s "$TRANSCRIPT_FILE" ]; then
        echo -e "${GREEN}Transcript already exists, using cached version...${NC}"
        TRANSCRIPT=$(cat "$TRANSCRIPT_FILE")
    else
        # Check if audio file already exists
        if [ -f "$AUDIO_FILE" ]; then
            echo -e "${GREEN}Audio file already exists, using cached version...${NC}"
        else
            echo -e "${YELLOW}📢 Extracting audio...${NC}"
            ffmpeg -i "$VIDEO_FILE" -vn -acodec mp3 -ab 32k -ar 16000 -ac 1 -y "$AUDIO_FILE" 2>/dev/null

            if [ $? -ne 0 ]; then
                echo -e "${RED}❌ Failed to extract audio${NC}"
                exit 1
            fi
            echo -e "${GREEN}✅ Audio extracted to: $AUDIO_FILE${NC}"
        fi

        # Check file size and compress if needed
        FILE_SIZE=$(stat -f%z "$AUDIO_FILE" 2>/dev/null || stat -c%s "$AUDIO_FILE" 2>/dev/null)
        FILE_SIZE_MB=$((FILE_SIZE / 1024 / 1024))

        if [ $FILE_SIZE_MB -gt 24 ]; then
            echo -e "${YELLOW}⚠️ File too large, compressing...${NC}"
            COMPRESSED_AUDIO="${TMP_DIR}/${VIDEO_NAME}_compressed.mp3"
            ffmpeg -i "$AUDIO_FILE" -acodec mp3 -ab 24k -ar 16000 -ac 1 -y "$COMPRESSED_AUDIO" 2>/dev/null
            mv "$COMPRESSED_AUDIO" "$AUDIO_FILE"
            FILE_SIZE=$(stat -f%z "$AUDIO_FILE" 2>/dev/null || stat -c%s "$AUDIO_FILE" 2>/dev/null)
            FILE_SIZE_MB=$((FILE_SIZE / 1024 / 1024))
            echo -e "${GREEN}✅ Compressed to ${FILE_SIZE_MB}MB${NC}"
        fi

        # Using OpenAI API with gpt-4o-transcribe model
        echo -e "${YELLOW}Calling OpenAI gpt-4o-transcribe API...${NC}"
        TRANSCRIPTION_RESPONSE=$(curl -s -w "\nHTTP_STATUS:%{http_code}" https://api.openai.com/v1/audio/transcriptions \
          -H "Authorization: Bearer $OPENAI_API_KEY" \
          -H "Content-Type: multipart/form-data" \
          -F file="@$AUDIO_FILE" \
          -F model="gpt-4o-transcribe" \
          -F response_format="json" \
          -F language="zh")

        # Extract HTTP status
        HTTP_STATUS=$(echo "$TRANSCRIPTION_RESPONSE" | grep "HTTP_STATUS:" | cut -d: -f2)
        RESPONSE_BODY=$(echo "$TRANSCRIPTION_RESPONSE" | sed '/HTTP_STATUS:/d')

        if [ "$HTTP_STATUS" != "200" ]; then
            echo -e "${RED}❌ API call failed with status: $HTTP_STATUS${NC}"
            echo -e "${RED}Response: $RESPONSE_BODY${NC}"
            exit 1
        fi

        # Extract text from JSON response
        TRANSCRIPT=$(echo "$RESPONSE_BODY" | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    text = data.get('text', '')
    if text:
        print(text)
    else:
        print('Error: No text in response', file=sys.stderr)
        sys.exit(1)
except Exception as e:
    print(f'Error parsing JSON: {e}', file=sys.stderr)
    sys.exit(1)
")

        if [ -z "$TRANSCRIPT" ]; then
            echo -e "${RED}❌ Failed to extract transcript${NC}"
            exit 1
        fi

        echo "$TRANSCRIPT" > "$TRANSCRIPT_FILE"
        echo -e "${GREEN}✅ Transcript saved to: $TRANSCRIPT_FILE${NC}"
    fi
fi

echo -e "${GREEN}📄 Transcript preview:${NC}"
echo "$TRANSCRIPT" | head -n 5
echo "..."

# Step 3: Generate content using gpt-5-mini
echo -e "${YELLOW}🤖 Generating titles and descriptions with gpt-5-mini...${NC}"
CONTENT_FILE="${TMP_DIR}/${VIDEO_NAME}_content.json"

# Create prompt for GPT
PROMPT="基于以下视频转录内容，请生成：
1. 3个吸引人的标题选项（每个不超过16个字）
2. 2段详细的视频描述（每段300-500字）
3. 3个bilibili动态更新文案（每个150-250字）

请以JSON格式返回，格式如下：
{
  \"titles\": [\"标题1\", \"标题2\", \"标题3\"],
  \"descriptions\": [\"描述1\", \"描述2\"],
  \"status_updates\": [\"动态1\", \"动态2\", \"动态3\"]
}

转录内容：
$TRANSCRIPT"

# Create a temporary file for the request body to avoid shell escaping issues
REQUEST_BODY_FILE="${TMP_DIR}/${VIDEO_NAME}_request.json"

# Create the JSON request using Python for proper escaping
python3 <<EOF > "$REQUEST_BODY_FILE"
import json
import sys

prompt = '''$PROMPT'''

request = {
    "model": "gpt-5-mini",
    "messages": [
        {
            "role": "system",
            "content": "你是一个专业的内容创作助手，擅长为视频内容生成吸引人的标题和描述。请用中文回复，并严格按照JSON格式输出。"
        },
        {
            "role": "user",
            "content": prompt
        }
    ],
    "temperature": 1,
    "max_completion_tokens": 10000,
    "response_format": {"type": "json_object"}
}

print(json.dumps(request, ensure_ascii=False))
EOF

echo -e "${YELLOW}Calling gpt-5-mini API...${NC}"
GPT_RESPONSE=$(curl -s -w "\nHTTP_STATUS:%{http_code}" https://api.openai.com/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -d @"$REQUEST_BODY_FILE")

# Extract HTTP status
HTTP_STATUS=$(echo "$GPT_RESPONSE" | grep "HTTP_STATUS:" | cut -d: -f2)
RESPONSE_BODY=$(echo "$GPT_RESPONSE" | sed '/HTTP_STATUS:/d')

# Clean up request file
rm -f "$REQUEST_BODY_FILE"

if [ "$HTTP_STATUS" != "200" ]; then
    echo -e "${RED}❌ GPT-5-mini API call failed with status: $HTTP_STATUS${NC}"
    echo -e "${RED}Response: $RESPONSE_BODY${NC}"

    # Save error for debugging
    echo "$RESPONSE_BODY" > "${TMP_DIR}/${VIDEO_NAME}_error.json"
    exit 1
fi

# Extract content from GPT response
CONTENT=$(echo "$RESPONSE_BODY" | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    if 'choices' in data and len(data['choices']) > 0:
        content = data['choices'][0]['message']['content']
        # Try to parse as JSON
        try:
            json_content = json.loads(content)
            print(json.dumps(json_content, ensure_ascii=False, indent=2))
        except:
            # If not valid JSON, save as text
            print(content)
    else:
        print('Error: Invalid response')
except Exception as e:
    print(f'Error: {e}')
")

echo "$CONTENT" > "$CONTENT_FILE"
echo -e "${GREEN}✅ Content saved to: $CONTENT_FILE${NC}"

# Save individual files for each type of content
TITLES_FILE="${TMP_DIR}/${VIDEO_NAME}_titles.txt"
DESCRIPTIONS_FILE="${TMP_DIR}/${VIDEO_NAME}_descriptions.txt"
STATUS_FILE="${TMP_DIR}/${VIDEO_NAME}_status.txt"

# Extract and save titles
echo "$CONTENT" | python3 -c "
import sys, json
try:
    data = json.loads(sys.stdin.read())
    if 'titles' in data:
        for i, title in enumerate(data['titles'], 1):
            print(f'{i}. {title}')
except:
    pass
" > "$TITLES_FILE"

# Extract and save descriptions
echo "$CONTENT" | python3 -c "
import sys, json
try:
    data = json.loads(sys.stdin.read())
    if 'descriptions' in data:
        for i, desc in enumerate(data['descriptions'], 1):
            print(f'=== 描述 {i} ===')
            print(desc)
            print()
except:
    pass
" > "$DESCRIPTIONS_FILE"

# Extract and save status updates
echo "$CONTENT" | python3 -c "
import sys, json
try:
    data = json.loads(sys.stdin.read())
    if 'status_updates' in data:
        for i, status in enumerate(data['status_updates'], 1):
            print(f'=== 动态 {i} ===')
            print(status)
            print()
except:
    pass
" > "$STATUS_FILE"

# Display summary
echo -e "${GREEN}🎉 Processing complete!${NC}"
echo -e "${GREEN}Generated files:${NC}"
echo "  📝 Transcript: $TRANSCRIPT_FILE"
echo "  📋 Full content: $CONTENT_FILE"
echo "  🏷️ Titles: $TITLES_FILE"
echo "  📄 Descriptions: $DESCRIPTIONS_FILE"
echo "  💬 Status updates: $STATUS_FILE"

# Display preview of titles
echo -e "\n${YELLOW}Preview of generated titles:${NC}"
cat "$TITLES_FILE" 2>/dev/null || echo "Failed to display titles"

echo -e "\n${GREEN}✨ All files saved in /tmp directory${NC}"
