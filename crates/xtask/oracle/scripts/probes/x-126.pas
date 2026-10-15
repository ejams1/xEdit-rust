{ parity probe: x-126 }
unit P_x_126;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  AddMessage(IntToStr(Length(5)));
  AddMessage('after');
end;
end.
