{ parity probe: x-279 }
unit P_x_279;

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
  v := sl.Text; AddMessage('[' + v + ']');
  AddMessage('after');
end;
end.
