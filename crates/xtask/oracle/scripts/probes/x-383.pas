{ parity probe: x-383 }
unit P_x_383;

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
  v := TList.Create; v.Add(5); AddMessage(IntToStr(Integer(v.Items[0])));
  AddMessage('after');
end;
end.
