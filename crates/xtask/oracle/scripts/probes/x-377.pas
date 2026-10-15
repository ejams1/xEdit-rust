{ parity probe: x-377 }
unit P_x_377;

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
  v := TList.Create; v.Add(nil); AddMessage(IntToStr(v.IndexOf(nil)));
  AddMessage('after');
end;
end.
