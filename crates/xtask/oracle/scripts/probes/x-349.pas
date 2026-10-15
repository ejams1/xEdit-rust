{ parity probe: x-349 }
unit P_x_349;

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
  v := TList.Create; v.Add(sl); AddMessage(IntToStr(v.IndexOf(sl)));
  AddMessage('after');
end;
end.
