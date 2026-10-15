{ parity probe: x-248 }
unit P_x_248;

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
  v := ElementCount(True);
  AddMessage('after');
end;
end.
