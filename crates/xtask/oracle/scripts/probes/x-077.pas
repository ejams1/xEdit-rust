{ parity probe: x-077 }
unit P_x_077;

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
  v := Chr(256); AddMessage(IntToStr(Ord(v)));
  AddMessage('after');
end;
end.
