{ parity probe: x-258 }
unit P_x_258;

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
  v := IntToStr(Unassigned); AddMessage(v);
  AddMessage('after');
end;
end.
