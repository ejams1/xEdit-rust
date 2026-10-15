{ parity probe: x-337 }
unit P_x_337;

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
  v := TStringList.Create.Count; AddMessage(IntToStr(v));
  AddMessage('after');
end;
end.
