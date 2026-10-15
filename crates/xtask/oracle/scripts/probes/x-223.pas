{ parity probe: x-223 }
unit P_x_223;

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
  v := Assigned(Null); AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
