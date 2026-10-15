{ parity probe: x-366 }
unit P_x_366;

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
  v := TList.Create; v.Add(5); v.Pack; AddMessage(IntToStr(v.Count));
  AddMessage('after');
end;
end.
