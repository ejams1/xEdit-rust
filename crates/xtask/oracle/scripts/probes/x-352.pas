{ parity probe: x-352 }
unit P_x_352;

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
  v := TList.Create; v.Count := 3; AddMessage(IntToStr(v.Count));
  AddMessage('after');
end;
end.
