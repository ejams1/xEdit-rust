{ parity probe: x-341 }
unit P_x_341;

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
  v := TList.Create; v.Add(nil); AddMessage(IntToStr(v.Count));
  AddMessage('after');
end;
end.
